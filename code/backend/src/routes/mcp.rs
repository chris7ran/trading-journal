//! The journal as an MCP server: JSON-RPC over HTTP, written by hand.
//!
//! No SDK crate. The surface actually needed is small — `initialize`, a tool
//! list, a tool call, `ping`, and swallowing notifications — and a protocol
//! crate would bring a dependency tree, a release cadence and a set of
//! abstractions far larger than the two hundred lines below. The MT5 bridge
//! on the Windows side uses the Python SDK because it needs session handling
//! this does not.
//!
//! **Stateless by design.** No `Mcp-Session-Id` is issued, so no session
//! state has to survive a restart of the container and a client reconnecting
//! mid-conversation simply works. The spec allows it: a server that returns
//! no session id is never sent one back.
//!
//! **Read-only.** Every tool reads. Nothing here creates, edits or deletes a
//! trade, an account or a rule — the journal is written by the app, by the
//! CSV importer and by the MT5 feed, all of which authenticate differently.
//! An assistant that can read the whole journal is useful; one that can
//! rewrite it is a liability.
//!
//! Tools delegate to the same handlers the REST routes call, rather than
//! issuing their own SQL. Two code paths answering "what are my stats" would
//! eventually disagree, and the one nobody looks at would be the wrong one.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};
use crate::models::TradeFilters;
use crate::routes::{accounts, archive, levels, setups, stats, trades};
use crate::state::AppState;

const SERVER_NAME: &str = "trading-journal";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Protocol revision we answer with when the client asks for one we do not
/// know. Clients are expected to either accept it or disconnect.
const PREFERRED_PROTOCOL: &str = "2025-06-18";

/// Revisions we are happy to speak. Nothing in this server uses a feature
/// that differs between them, so the list is about not refusing a client
/// over a version string.
const SUPPORTED_PROTOCOLS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// Told to the model once, at connection. Short on purpose: a long preamble
/// is paid for on every request and read carefully on none.
const INSTRUCTIONS: &str = "\
Journal de trading personnel, en lecture seule. Comptes, trades, statistiques, \
règles prop firm, niveaux du jour et briefs archivés. Aucun outil ne modifie \
quoi que ce soit.\n\n\
Commencez par list_accounts : la plupart des outils prennent un account_id, et \
les comptes n'ont pas tous la même devise ni le même solde de départ. \
get_stats accepte un group_by (day, week, month, setup, direction, account, \
plan, symbol) — c'est lui qui transforme un chiffre global en question utile.";

// --- Authentication ----------------------------------------------------------

const AUTH_HEADER: &str = "authorization";

/// Guard every `/mcp` request.
///
/// A layer rather than a check inside the handler, for the same reason the
/// ingest routes do it: an unauthenticated request must be refused before its
/// body is buffered and parsed, and must get a 401 rather than a parse error
/// that would describe the endpoint's shape to someone with no business
/// knowing it.
pub async fn require_mcp_token(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, AppError> {
    check_token(&state, req.headers())?;
    Ok(next.run(req).await)
}

fn check_token(state: &AppState, headers: &HeaderMap) -> AppResult<()> {
    // Trim before the emptiness check: a whitespace-only MCP_TOKEN must read
    // as "not configured", or a stray space in the env file would turn into
    // an open read endpoint that any empty header matches.
    let expected = state.config.mcp_token.trim().as_bytes();
    if expected.is_empty() {
        tracing::warn!("/mcp appelé mais MCP_TOKEN n'est pas configuré — rejet");
        return Err(AppError::Unauthorized);
    }

    let provided = headers
        .get(AUTH_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(AppError::Unauthorized)?;

    if constant_time_eq(provided.trim().as_bytes(), expected) {
        Ok(())
    } else {
        Err(AppError::Unauthorized)
    }
}

/// Compare two secrets without returning early on the first differing byte.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// --- JSON-RPC ----------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RpcRequest {
    /// Present on a request, absent on a notification. That absence is the
    /// whole difference, and it decides whether anything may be sent back.
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

struct RpcError {
    code: i32,
    message: String,
}

impl RpcError {
    fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("méthode inconnue : {method}"),
        }
    }

    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
        }
    }
}

/// `GET /mcp` — we never push, so there is no stream to open.
///
/// Answering 405 rather than 404 matters: a client that gets 404 concludes
/// the endpoint does not exist, while 405 tells it the endpoint is there and
/// simply does not take a GET, which is exactly the case.
pub async fn stream_not_supported() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(json!({ "error": "ce serveur ne pousse rien : utilisez POST" })),
    )
        .into_response()
}

/// `POST /mcp` — the whole protocol.
pub async fn handle(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let request: RpcRequest = match serde_json::from_value(body) {
        Ok(request) => request,
        Err(e) => {
            return rpc_error(
                Value::Null,
                RpcError {
                    code: -32600,
                    message: format!("requête JSON-RPC invalide : {e}"),
                },
            )
        }
    };

    // A notification carries no id and must get no response body. Returning
    // one would leave a client that correlates on id waiting for an answer
    // to a question it never asked.
    if request.id.is_none() {
        return StatusCode::ACCEPTED.into_response();
    }
    let id = request.id.clone().unwrap_or(Value::Null);

    match dispatch(&state, &request.method, request.params).await {
        Ok(result) => Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response(),
        Err(error) => rpc_error(id, error),
    }
}

fn rpc_error(id: Value, error: RpcError) -> Response {
    Json(json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": error.code, "message": error.message },
    }))
    .into_response()
}

async fn dispatch(state: &AppState, method: &str, params: Option<Value>) -> Result<Value, RpcError> {
    match method {
        "initialize" => Ok(initialize(params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_definitions() })),
        "tools/call" => call_tool(state, params).await,
        other => Err(RpcError::method_not_found(other)),
    }
}

fn initialize(params: Option<Value>) -> Value {
    let requested = params
        .as_ref()
        .and_then(|p| p.get("protocolVersion"))
        .and_then(Value::as_str);

    let version = match requested {
        Some(v) if SUPPORTED_PROTOCOLS.contains(&v) => v,
        _ => PREFERRED_PROTOCOL,
    };

    json!({
        "protocolVersion": version,
        // Only tools. No resources, no prompts, no sampling: the journal has
        // nothing to offer on those, and advertising them would invite calls
        // that can only fail.
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
        "instructions": INSTRUCTIONS,
    })
}

// --- Tools -------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CallParams {
    name: String,
    #[serde(default)]
    arguments: Option<Value>,
}

async fn call_tool(state: &AppState, params: Option<Value>) -> Result<Value, RpcError> {
    let params: CallParams = serde_json::from_value(params.unwrap_or(Value::Null))
        .map_err(|e| RpcError::invalid_params(format!("paramètres de tools/call invalides : {e}")))?;

    let arguments = params.arguments.unwrap_or_else(|| json!({}));

    // A failing tool is reported as a *successful* JSON-RPC result carrying
    // `isError`, not as a protocol error. That is the MCP convention, and it
    // is the useful one: the model sees the message and can correct itself,
    // where a transport-level error would just end the exchange.
    match run_tool(state, &params.name, &arguments).await {
        Ok(value) => {
            let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
            Ok(json!({
                "content": [{ "type": "text", "text": text }],
                "structuredContent": value,
                "isError": false,
            }))
        }
        Err(error) => Ok(json!({
            "content": [{ "type": "text", "text": error.to_string() }],
            "isError": true,
        })),
    }
}

async fn run_tool(state: &AppState, name: &str, args: &Value) -> AppResult<Value> {
    match name {
        "list_accounts" => {
            let accounts = accounts::list_accounts(State(state.clone())).await?.0;
            to_value(json!({ "count": accounts.len(), "accounts": accounts }))
        }

        "get_trades" => {
            let filters = TradeFilters {
                account_id: opt_str(args, "account_id"),
                symbol: opt_str(args, "symbol"),
                direction: opt_str(args, "direction"),
                from: opt_str(args, "from"),
                to: opt_str(args, "to"),
                limit: opt_i64(args, "limit"),
                offset: opt_i64(args, "offset"),
            };
            let trades = trades::list_trades(State(state.clone()), Query(filters))
                .await?
                .0;
            to_value(json!({ "count": trades.len(), "trades": trades }))
        }

        "get_stats" => {
            let filters = stats::StatsFilters {
                account_id: opt_str(args, "account_id"),
                symbol: opt_str(args, "symbol"),
                from: opt_str(args, "from"),
                to: opt_str(args, "to"),
                group_by: opt_str(args, "group_by"),
            };
            let stats = stats::trade_stats(State(state.clone()), Query(filters))
                .await?
                .0;
            to_value(stats)
        }

        "get_prop_status" => {
            let account_id = require_str(args, "account_id")?;
            to_value(accounts::load_prop_status(state, &account_id).await?)
        }

        "get_levels" => {
            let query = levels::LevelsQuery {
                symbol: require_str(args, "symbol")?,
                date: opt_str(args, "date"),
            };
            to_value(levels::get_levels(State(state.clone()), Query(query)).await?.0)
        }

        "get_brief" => {
            let query = levels::LevelsQuery {
                symbol: require_str(args, "symbol")?,
                date: opt_str(args, "date"),
            };
            to_value(levels::get_brief(State(state.clone()), Query(query)).await?.0)
        }

        "get_brief_review" => {
            let query = archive::ReviewQuery {
                symbol: opt_str(args, "symbol"),
                from: opt_str(args, "from"),
                to: opt_str(args, "to"),
            };
            let days = archive::review(State(state.clone()), Query(query)).await?.0;
            to_value(json!({ "count": days.len(), "days": days }))
        }

        "list_symbols" => Ok(levels::list_symbols(State(state.clone())).await?.0),

        "list_setups" => {
            let setups = setups::list_setups(State(state.clone())).await?.0;
            to_value(json!({ "count": setups.len(), "setups": setups }))
        }

        other => Err(AppError::BadRequest(format!("outil inconnu : {other}"))),
    }
}

/// Every tool, with the schema a client validates arguments against.
fn tool_definitions() -> Vec<Value> {
    vec![
        tool(
            "list_accounts",
            "Lister les comptes du journal : nom, broker, devise, solde de départ, \
             et le terminal MT5 auquel chacun est relié s'il y en a un. \
             À appeler en premier, la plupart des autres outils prennent un account_id.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        tool(
            "get_trades",
            "Lister les trades, du plus récent au plus ancien. Sans filtre, les 100 derniers. \
             Les dates sont des bornes sur l'ouverture, au format YYYY-MM-DD ou ISO-8601.",
            json!({
                "type": "object",
                "properties": {
                    "account_id": { "type": "string", "description": "identifiant renvoyé par list_accounts" },
                    "symbol": { "type": "string" },
                    "direction": { "type": "string", "enum": ["LONG", "SHORT"] },
                    "from": { "type": "string", "description": "borne basse incluse" },
                    "to": { "type": "string", "description": "borne haute incluse" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 1000 },
                    "offset": { "type": "integer", "minimum": 0 }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "get_stats",
            "Statistiques agrégées : nombre de trades, taux de réussite, P&L, facteur de profit, \
             gain et perte moyens. group_by ajoute une ventilation sur une dimension — c'est lui \
             qui transforme un chiffre global en question utile : un taux de 58 % ne dit rien, \
             le même découpé par setup dit lequel porte les autres.",
            json!({
                "type": "object",
                "properties": {
                    "account_id": { "type": "string" },
                    "symbol": { "type": "string" },
                    "from": { "type": "string" },
                    "to": { "type": "string" },
                    "group_by": {
                        "type": "string",
                        "enum": ["symbol", "day", "week", "month", "setup", "direction", "account", "plan"]
                    }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "get_prop_status",
            "Règles prop firm évaluées sur un compte : objectif de profit, drawdown global depuis \
             le plus haut, perte journalière, règle de consistance, et les alertes qui en découlent. \
             Les seuils sont des pourcentages du solde de départ, pas de l'équité courante.",
            json!({
                "type": "object",
                "properties": { "account_id": { "type": "string" } },
                "required": ["account_id"],
                "additionalProperties": false
            }),
        ),
        tool(
            "get_levels",
            "Niveaux d'une journée pour un symbole : haut et bas du jour et de la veille, \
             0,5 de la veille, ranges de sessions, ORB de Londres et de New York, et les \
             balayages détectés. La date est un jour calendaire parisien.",
            json!({
                "type": "object",
                "properties": {
                    "symbol": { "type": "string" },
                    "date": { "type": "string", "description": "YYYY-MM-DD, par défaut aujourd'hui" }
                },
                "required": ["symbol"],
                "additionalProperties": false
            }),
        ),
        tool(
            "get_brief",
            "Les niveaux plus le contexte qui leur donne un sens : amplitude journalière moyenne, \
             position du jour contre cette moyenne, biais daily lu sur le 0,5 de la veille, \
             profil hebdomadaire, et distance de chaque niveau non pris en fraction de journée moyenne.",
            json!({
                "type": "object",
                "properties": {
                    "symbol": { "type": "string" },
                    "date": { "type": "string", "description": "YYYY-MM-DD, par défaut aujourd'hui" }
                },
                "required": ["symbol"],
                "additionalProperties": false
            }),
        ),
        tool(
            "get_brief_review",
            "Les briefs figés avant séance, à côté des trades réellement pris ce jour-là. \
             Sert à mesurer après coup si la lecture du matin valait quelque chose, \
             au lieu de la juger de mémoire.",
            json!({
                "type": "object",
                "properties": {
                    "symbol": { "type": "string" },
                    "from": { "type": "string", "description": "YYYY-MM-DD, par défaut 30 jours en arrière" },
                    "to": { "type": "string", "description": "YYYY-MM-DD" }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "list_symbols",
            "Symboles pour lesquels le flux MT5 a effectivement livré des bougies, \
             avec la première et la dernière reçues. Dit aussi si le flux est à jour.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        tool(
            "list_setups",
            "Setups définis dans le journal, avec leurs règles d'entrée, de sortie et de stop. \
             Le champ setup_tag d'un trade y renvoie.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
    ]
}

fn tool(name: &str, description: &str, schema: Value) -> Value {
    json!({ "name": name, "description": description, "inputSchema": schema })
}

// --- Argument helpers ---------------------------------------------------------

/// A string argument, treating empty and whitespace as absent.
///
/// Models routinely pass `""` for "no filter"; letting that through would
/// turn an unfiltered question into a search for the empty symbol.
fn opt_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn require_str(args: &Value, key: &str) -> AppResult<String> {
    opt_str(args, key).ok_or_else(|| AppError::BadRequest(format!("argument '{key}' requis")))
}

/// An integer argument, accepting the string form too — a schema says
/// `integer`, but a model that has just written JSON by hand often sends
/// `"50"`, and failing on that helps nobody.
fn opt_i64(args: &Value, key: &str) -> Option<i64> {
    let value = args.get(key)?;
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
}

fn to_value<T: Serialize>(value: T) -> AppResult<Value> {
    serde_json::to_value(value).map_err(|e| AppError::Other(anyhow::anyhow!(e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_protocol_version_is_echoed_back() {
        let result = initialize(Some(json!({ "protocolVersion": "2025-03-26" })));
        assert_eq!(result["protocolVersion"], "2025-03-26");
    }

    #[test]
    fn an_unknown_protocol_version_falls_back_instead_of_failing() {
        // Refusing would strand a newer client over a string, when nothing
        // this server does differs between revisions.
        let result = initialize(Some(json!({ "protocolVersion": "2099-01-01" })));
        assert_eq!(result["protocolVersion"], PREFERRED_PROTOCOL);
        let result = initialize(None);
        assert_eq!(result["protocolVersion"], PREFERRED_PROTOCOL);
    }

    #[test]
    fn the_server_advertises_tools_and_nothing_else() {
        let result = initialize(None);
        assert!(result["capabilities"]["tools"].is_object());
        assert!(result["capabilities"]["resources"].is_null());
        assert!(result["capabilities"]["prompts"].is_null());
    }

    #[test]
    fn every_tool_has_a_name_a_description_and_a_schema() {
        let tools = tool_definitions();
        assert!(!tools.is_empty());
        for definition in &tools {
            let name = definition["name"].as_str().expect("un nom");
            assert!(!name.is_empty());
            assert!(
                definition["description"].as_str().is_some_and(|d| d.len() > 30),
                "{name} : description trop courte pour guider un modèle"
            );
            assert_eq!(definition["inputSchema"]["type"], "object", "{name}");
        }
    }

    #[test]
    fn tool_names_are_unique() {
        let tools = tool_definitions();
        let mut names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "deux outils portent le même nom");
    }

    #[test]
    fn required_arguments_appear_in_their_schema() {
        // A required argument the schema does not declare is one the client
        // will not ask the model for, and the call fails at run time instead.
        for definition in tool_definitions() {
            let name = definition["name"].as_str().unwrap().to_string();
            let required: Vec<String> = definition["inputSchema"]["required"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let properties = &definition["inputSchema"]["properties"];
            for key in required {
                assert!(
                    properties.get(&key).is_some(),
                    "{name} : '{key}' est requis mais absent des propriétés"
                );
            }
        }
    }

    #[test]
    fn an_empty_string_argument_counts_as_absent() {
        let args = json!({ "symbol": "  ", "account_id": "abc" });
        assert_eq!(opt_str(&args, "symbol"), None);
        assert_eq!(opt_str(&args, "account_id").as_deref(), Some("abc"));
        assert_eq!(opt_str(&args, "manquant"), None);
    }

    #[test]
    fn an_integer_sent_as_a_string_is_still_an_integer() {
        assert_eq!(opt_i64(&json!({ "limit": 50 }), "limit"), Some(50));
        assert_eq!(opt_i64(&json!({ "limit": "50" }), "limit"), Some(50));
        assert_eq!(opt_i64(&json!({ "limit": "beaucoup" }), "limit"), None);
        assert_eq!(opt_i64(&json!({}), "limit"), None);
    }

    #[test]
    fn a_missing_required_argument_names_itself() {
        let error = require_str(&json!({}), "symbol").unwrap_err().to_string();
        assert!(error.contains("symbol"));
    }

    #[test]
    fn secrets_are_compared_without_an_early_return() {
        assert!(constant_time_eq(b"jeton", b"jeton"));
        assert!(!constant_time_eq(b"jeton", b"jetoo"));
        assert!(!constant_time_eq(b"jeton", b"jeton-plus-long"));
    }
}
