//! Macro terminal: economic calendar (red/orange impact) and aggregated news.
//!
//! Data sources (free, no API key):
//! - Calendar: faireconomy's mirror of the Forex Factory weekly calendar (JSON).
//! - News: a small list of public RSS feeds, parsed with `feed-rs`.
//!
//! Both endpoints fetch at request time. Failures degrade gracefully (the news
//! endpoint skips feeds it can't reach; the calendar returns a clear error).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::Json;
use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::models::{CotEntry, EcoEvent, EconIndicator, NewsItem};

// In-memory caches. faireconomy rate-limits to 2 downloads / 5 min, so we serve
// a cached copy and only refresh after the TTL — and fall back to stale data if
// a live refresh is rate-limited or fails.
const CALENDAR_TTL: Duration = Duration::from_secs(15 * 60);
const NEWS_TTL: Duration = Duration::from_secs(10 * 60);
const ECON_TTL: Duration = Duration::from_secs(12 * 60 * 60); // annual data — refresh rarely
const COT_TTL: Duration = Duration::from_secs(6 * 60 * 60); // CFTC publishes weekly

static CALENDAR_CACHE: Mutex<Option<(Instant, Vec<EcoEvent>)>> = Mutex::new(None);
static NEWS_CACHE: Mutex<Option<(Instant, Vec<NewsItem>)>> = Mutex::new(None);
static ECON_CACHE: Mutex<Option<(Instant, Vec<EconIndicator>)>> = Mutex::new(None);
static COT_CACHE: Mutex<Option<(Instant, Vec<CotEntry>)>> = Mutex::new(None);

// World Bank indicators (free, no key): (label, region, ISO3, code, unit).
const WB_INDICATORS: &[(&str, &str, &str, &str, &str)] = &[
    (
        "Inflation (CPI)",
        "États-Unis",
        "USA",
        "FP.CPI.TOTL.ZG",
        "%",
    ),
    ("Chômage", "États-Unis", "USA", "SL.UEM.TOTL.ZS", "%"),
    (
        "Croissance PIB",
        "États-Unis",
        "USA",
        "NY.GDP.MKTP.KD.ZG",
        "%",
    ),
    ("Inflation (CPI)", "Zone euro", "EMU", "FP.CPI.TOTL.ZG", "%"),
    (
        "Croissance PIB",
        "Allemagne",
        "DEU",
        "NY.GDP.MKTP.KD.ZG",
        "%",
    ),
    ("Chômage", "Allemagne", "DEU", "SL.UEM.TOTL.ZS", "%"),
    ("Inflation (CPI)", "Japon", "JPN", "FP.CPI.TOTL.ZG", "%"),
    ("Chômage", "Japon", "JPN", "SL.UEM.TOTL.ZS", "%"),
    ("Croissance PIB", "Japon", "JPN", "NY.GDP.MKTP.KD.ZG", "%"),
];

// faireconomy mirror of the Forex Factory weekly calendar (free, no key).
const CALENDAR_URLS: &[&str] = &["https://nfs.faireconomy.media/ff_calendar_thisweek.json"];

// Public RSS feeds by theme (no key). Investing.com: news_1=Forex, news_95=macro
// indicators, news_25=stock/indices. MarketWatch = US/Wall Street. The rest are
// broker/analysis feeds and central-bank press wires (Fed/ECB/BoE/BoJ).
// Skipped after live checks: DailyFX (HTTP 403, Akamai-blocked) and the CNBC
// combinednewsletter feed (returns an empty channel, 0 items).
const NEWS_FEEDS: &[(&str, &str)] = &[
    (
        "Investing · Forex",
        "https://www.investing.com/rss/news_1.rss",
    ),
    (
        "Investing · Macro",
        "https://www.investing.com/rss/news_95.rss",
    ),
    (
        "Investing · Indices",
        "https://www.investing.com/rss/news_25.rss",
    ),
    (
        "MarketWatch",
        "https://feeds.content.dowjones.io/public/rss/mw_topstories",
    ),
    // ForexLive redirects to its new investinglive.com home; use it directly.
    ("ForexLive", "https://investinglive.com/feed/news"),
    ("FXStreet", "https://www.fxstreet.com/rss/news"),
    ("Fed", "https://www.federalreserve.gov/feeds/press_all.xml"),
    ("BCE", "https://www.ecb.europa.eu/rss/press.html"),
    ("BoE", "https://www.bankofengland.co.uk/rss/news"),
    ("BoJ", "https://www.boj.or.jp/en/rss/whatsnew.xml"),
];

const USER_AGENT: &str = "Mozilla/5.0 (compatible; TradingJournal/0.1; +https://example.local)";

fn http_client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| AppError::Other(anyhow::anyhow!("http client: {e}")))
}

/// Raw row from the faireconomy calendar feed.
#[derive(Debug, Deserialize)]
struct FfEvent {
    title: String,
    country: String,
    date: String,
    impact: String,
    forecast: Option<String>,
    previous: Option<String>,
}

/// Fetch the calendar JSON text, trying the CDN fallback, and guarding against
/// the rate-limit "Request Denied" HTML page (non-JSON) faireconomy can return.
/// Returns a detailed reason on failure (logged for diagnosis).
async fn fetch_calendar_text(client: &reqwest::Client) -> Result<String, String> {
    let mut last = "aucune tentative".to_string();
    for url in CALENDAR_URLS.iter().copied() {
        match client.get(url).send().await {
            Ok(resp) => {
                let status = resp.status();
                match resp.text().await {
                    Ok(text) => {
                        if text.trim_start().starts_with('[') {
                            return Ok(text);
                        }
                        let head: String = text.chars().take(80).collect();
                        last = format!("{url}: HTTP {status}, corps inattendu « {head} »");
                    }
                    Err(e) => last = format!("{url}: lecture du corps impossible: {e}"),
                }
            }
            Err(e) => last = format!("{url}: requête échouée: {e}"),
        }
    }
    Err(last)
}

/// Fetch + parse the calendar fresh from faireconomy.
async fn fetch_calendar_events(client: &reqwest::Client) -> Result<Vec<EcoEvent>, String> {
    let text = fetch_calendar_text(client).await?;
    let raw: Vec<FfEvent> =
        serde_json::from_str(&text).map_err(|e| format!("calendrier illisible: {e}"))?;
    Ok(raw
        .into_iter()
        .filter_map(|e| {
            let impact = match e.impact.to_lowercase().as_str() {
                "high" => "red",
                "medium" => "orange",
                "low" => "yellow",
                _ => return None, // skip holidays / non-economic
            };
            Some(EcoEvent {
                title: e.title,
                currency: e.country,
                impact: impact.to_string(),
                date: e.date,
                forecast: e.forecast.filter(|s| !s.is_empty()),
                previous: e.previous.filter(|s| !s.is_empty()),
            })
        })
        .collect())
}

/// `GET /macro/calendar` — cached; refreshes after TTL, serves stale on failure.
pub async fn calendar() -> AppResult<Json<Vec<EcoEvent>>> {
    // Serve fresh cache without hitting the network.
    if let Ok(guard) = CALENDAR_CACHE.lock() {
        if let Some((ts, data)) = guard.as_ref() {
            if ts.elapsed() < CALENDAR_TTL {
                return Ok(Json(data.clone()));
            }
        }
    }

    let client = http_client()?;
    match fetch_calendar_events(&client).await {
        Ok(events) => {
            if let Ok(mut guard) = CALENDAR_CACHE.lock() {
                *guard = Some((Instant::now(), events.clone()));
            }
            Ok(Json(events))
        }
        Err(reason) => {
            tracing::warn!(%reason, "calendrier: refresh échoué, tentative cache");
            // Serve stale cache rather than failing (e.g. rate-limited).
            if let Ok(guard) = CALENDAR_CACHE.lock() {
                if let Some((_, data)) = guard.as_ref() {
                    return Ok(Json(data.clone()));
                }
            }
            Err(AppError::Other(anyhow::anyhow!(
                "calendrier injoignable — {reason}"
            )))
        }
    }
}

const BULLISH: &[&str] = &[
    "surge",
    "surges",
    "rally",
    "rallies",
    "jump",
    "jumps",
    "gain",
    "gains",
    "rise",
    "rises",
    "soar",
    "soars",
    "beat",
    "beats",
    "record high",
    "upgrade",
    "optimism",
    "boost",
    "strong",
    "rebound",
    "recover",
    "recovers",
    "climb",
    "climbs",
    "tops",
    "bullish",
    "outperform",
];
const BEARISH: &[&str] = &[
    "fall",
    "falls",
    "drop",
    "drops",
    "plunge",
    "plunges",
    "slump",
    "slumps",
    "crash",
    "fear",
    "fears",
    "recession",
    "downgrade",
    "miss",
    "misses",
    "weak",
    "selloff",
    "sell-off",
    "tumble",
    "tumbles",
    "warning",
    "warn",
    "sink",
    "sinks",
    "loss",
    "losses",
    "bearish",
    "slowdown",
    "decline",
    "declines",
    "cut",
    "cuts",
];

/// Very small lexicon-based sentiment for a headline.
fn sentiment_of(title: &str) -> &'static str {
    let t = title.to_lowercase();
    let bull = BULLISH.iter().filter(|w| t.contains(**w)).count() as i32;
    let bear = BEARISH.iter().filter(|w| t.contains(**w)).count() as i32;
    if bull > bear {
        "bullish"
    } else if bear > bull {
        "bearish"
    } else {
        "neutral"
    }
}

/// Fetch + parse news fresh from the RSS feeds.
async fn fetch_news_items(client: &reqwest::Client) -> Vec<NewsItem> {
    let mut items: Vec<NewsItem> = Vec::new();

    for (source, url) in NEWS_FEEDS {
        let bytes = match client.get(*url).send().await {
            Ok(resp) => match resp.bytes().await {
                Ok(b) => b,
                Err(_) => continue,
            },
            Err(_) => continue, // skip unreachable feed
        };

        let feed = match feed_rs::parser::parse(&bytes[..]) {
            Ok(f) => f,
            Err(_) => continue, // skip unparseable feed
        };

        for entry in feed.entries.into_iter().take(15) {
            let title = entry.title.map(|t| t.content).unwrap_or_default();
            let url = entry
                .links
                .first()
                .map(|l| l.href.clone())
                .unwrap_or_default();
            if title.is_empty() || url.is_empty() {
                continue;
            }
            let published_at = entry.published.or(entry.updated).map(|d| d.to_rfc3339());
            let sentiment = sentiment_of(&title).to_string();
            items.push(NewsItem {
                title,
                url,
                source: source.to_string(),
                published_at,
                sentiment,
            });
        }
    }

    // Newest first (entries without a date sink to the bottom).
    items.sort_by(|a, b| b.published_at.cmp(&a.published_at));
    items.truncate(40);
    items
}

/// `GET /macro/news` — cached; refreshes after TTL, serves stale on failure.
pub async fn news() -> AppResult<Json<Vec<NewsItem>>> {
    if let Ok(guard) = NEWS_CACHE.lock() {
        if let Some((ts, data)) = guard.as_ref() {
            if ts.elapsed() < NEWS_TTL {
                return Ok(Json(data.clone()));
            }
        }
    }

    let client = http_client()?;
    let items = fetch_news_items(&client).await;
    if !items.is_empty() {
        if let Ok(mut guard) = NEWS_CACHE.lock() {
            *guard = Some((Instant::now(), items.clone()));
        }
        return Ok(Json(items));
    }

    // All feeds failed: serve stale cache if we have one, else empty.
    if let Ok(guard) = NEWS_CACHE.lock() {
        if let Some((_, data)) = guard.as_ref() {
            return Ok(Json(data.clone()));
        }
    }
    Ok(Json(items))
}

/// Fetch macro indicators from the World Bank (annual, no key). Skips any
/// indicator that fails or has no data.
async fn fetch_economy(client: &reqwest::Client) -> Vec<EconIndicator> {
    let mut out: Vec<EconIndicator> = Vec::new();

    for (label, region, iso, code, unit) in WB_INDICATORS {
        let url = format!(
            "https://api.worldbank.org/v2/country/{iso}/indicator/{code}?format=json&per_page=8&mrv=8"
        );
        let value: serde_json::Value = match client.get(&url).send().await {
            Ok(resp) => match resp.json().await {
                Ok(v) => v,
                Err(_) => continue,
            },
            Err(_) => continue,
        };

        // Body is [ {metadata}, [ {date, value}, ... ] ] (newest first).
        let rows = match value.get(1).and_then(|v| v.as_array()) {
            Some(r) => r,
            None => continue,
        };

        // Keep (year, value) pairs where value is present.
        let pairs: Vec<(String, f64)> = rows
            .iter()
            .filter_map(|row| {
                let year = row.get("date")?.as_str()?.to_string();
                let v = row.get("value")?.as_f64()?;
                Some((year, v))
            })
            .collect();

        if pairs.is_empty() {
            continue;
        }

        let (year, value) = pairs[0].clone();
        let previous = pairs.get(1).map(|(_, v)| *v);
        // Oldest -> newest for the sparkline.
        let history: Vec<f64> = pairs.iter().rev().map(|(_, v)| *v).collect();

        out.push(EconIndicator {
            label: label.to_string(),
            region: region.to_string(),
            unit: unit.to_string(),
            year,
            value,
            previous,
            history,
            category: "macro".to_string(),
        });
    }

    out
}

// Market instruments: (label, region, unit, Stooq symbol, Yahoo symbol).
const MARKETS: &[(&str, &str, &str, &str, &str)] = &[
    ("Or (XAU/USD)", "Or", "$", "xauusd", "GC=F"),
    ("Pétrole (WTI)", "Pétrole", "$", "cl.f", "CL=F"),
    ("Rendement 10 ans US", "Taux US", "%", "10usy.b", "^TNX"),
    (
        "Fed · taux court US (proxy)",
        "Taux US",
        "%",
        "2usy.b",
        "^IRX",
    ),
];

// Browser-like UA: some market endpoints return empty for non-browser agents.
const MARKET_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36";

/// Daily (date, close) pairs from Stooq CSV, oldest -> newest. None if unavailable.
async fn stooq_series(client: &reqwest::Client, symbol: &str) -> Option<Vec<(String, f64)>> {
    let url = format!("https://stooq.com/q/d/l/?s={symbol}&i=d");
    let text = client
        .get(&url)
        .header(reqwest::header::USER_AGENT, MARKET_UA)
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
    if text.trim().is_empty() || text.contains("N/D") {
        return None;
    }
    let mut lines = text.lines();
    let header = lines.next()?;
    let cols: Vec<String> = header.split(',').map(|c| c.trim().to_lowercase()).collect();
    let date_idx = cols.iter().position(|c| c.as_str() == "date");
    let close_idx = cols.iter().position(|c| c.as_str() == "close")?;

    let mut pairs: Vec<(String, f64)> = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(',').collect();
        let close = match fields
            .get(close_idx)
            .and_then(|s| s.trim().parse::<f64>().ok())
        {
            Some(v) => v,
            None => continue,
        };
        let date = date_idx
            .and_then(|i| fields.get(i))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        pairs.push((date, close));
    }
    if pairs.is_empty() {
        None
    } else {
        Some(pairs)
    }
}

/// Daily (date, close) pairs from Yahoo Finance chart API, oldest -> newest.
async fn yahoo_series(client: &reqwest::Client, symbol: &str) -> Option<Vec<(String, f64)>> {
    let enc = symbol.replace('^', "%5E");
    let url =
        format!("https://query1.finance.yahoo.com/v8/finance/chart/{enc}?interval=1d&range=1mo");
    let v: serde_json::Value = client
        .get(&url)
        .header(reqwest::header::USER_AGENT, MARKET_UA)
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;

    let result = v.get("chart")?.get("result")?.get(0)?;
    let ts = result.get("timestamp").and_then(|t| t.as_array());
    let closes = result
        .get("indicators")?
        .get("quote")?
        .get(0)?
        .get("close")?
        .as_array()?;

    let mut pairs: Vec<(String, f64)> = Vec::new();
    for (i, c) in closes.iter().enumerate() {
        let close = match c.as_f64() {
            Some(x) => x,
            None => continue,
        };
        let date = ts
            .and_then(|arr| arr.get(i))
            .and_then(|e| e.as_i64())
            .map(|e| e.to_string())
            .unwrap_or_default();
        pairs.push((date, close));
    }
    if pairs.is_empty() {
        None
    } else {
        Some(pairs)
    }
}

/// Fetch market instruments, trying Stooq then Yahoo. Skips any that fail.
async fn fetch_markets(client: &reqwest::Client) -> Vec<EconIndicator> {
    let mut out: Vec<EconIndicator> = Vec::new();

    for (label, region, unit, stooq_sym, yahoo_sym) in MARKETS.iter().copied() {
        let (source, pairs) = match stooq_series(client, stooq_sym).await {
            Some(p) => ("stooq", p),
            None => match yahoo_series(client, yahoo_sym).await {
                Some(p) => ("yahoo", p),
                None => {
                    tracing::warn!(%label, "marché: aucune source disponible (Stooq + Yahoo)");
                    continue;
                }
            },
        };

        // Keep the most recent ~30 sessions.
        let start = pairs.len().saturating_sub(30);
        let recent = &pairs[start..];
        let (year, mut value) = recent[recent.len() - 1].clone();
        let mut previous = if recent.len() >= 2 {
            Some(recent[recent.len() - 2].1)
        } else {
            None
        };
        let mut history: Vec<f64> = recent.iter().map(|(_, v)| *v).collect();

        // Some yield feeds quote x10 (e.g. 42.5 for 4.25%). Normalise to percent.
        if unit == "%" && value > 20.0 {
            value /= 10.0;
            previous = previous.map(|p| p / 10.0);
            history = history.iter().map(|v| v / 10.0).collect();
        }

        tracing::info!(%label, %source, value, "marché: instrument récupéré");

        out.push(EconIndicator {
            label: label.to_string(),
            region: region.to_string(),
            unit: unit.to_string(),
            year,
            value,
            previous,
            history,
            category: "market".to_string(),
        });
    }

    tracing::info!(count = out.len(), "marché: total instruments");
    out
}

// FRED monthly macro series (needs a free API key in FRED_API_KEY):
// (label, region, unit, series_id, units). FRED `units`: pc1 = % change vs a year
// ago, lin = level, chg = change vs previous period. All IDs below were confirmed
// live via the keyless fredgraph.csv endpoint.
//
// Region labels match the mobile currency map ("Zone euro" lowercase e).
//
// Skipped (fredgraph 404, invalid IDs): the ECB HICP `CPALTT01EZM659N`, euro-area
// GDP `CLVMEURSCAB1GQEZ` and BoE `BOERATED` from the source list. The first two
// are replaced here by live equivalents (CP0000EZ19M086NEST, CLVMNACSCAB1GQEA19);
// no clean keyless replacement was found for the BoE Bank Rate.
//
// Skipped (discontinued OECD series, data frozen years ago): JP CPI
// `CPALTT01JPM659N` (last 2021-06) and BoJ discount rate `INTDSRJPM193N` (2017-04).
// The German/UK CPI series (CPALTT01*) are also OECD-MEI and end 2025-03 — kept as
// the latest available, but flagged.
const FRED_SERIES: &[(&str, &str, &str, &str, &str)] = &[
    // United States
    (
        "Inflation US (CPI a/a)",
        "États-Unis",
        "%",
        "CPIAUCSL",
        "pc1",
    ),
    ("Chômage US", "États-Unis", "%", "UNRATE", "lin"),
    ("Taux directeur Fed", "États-Unis", "%", "FEDFUNDS", "lin"),
    ("Croissance PIB US (a/a)", "États-Unis", "%", "GDPC1", "pc1"),
    ("Core CPI", "États-Unis", "%", "CPILFESL", "pc1"),
    ("Core PCE", "États-Unis", "%", "PCEPILFE", "pc1"),
    ("Nonfarm Payrolls", "États-Unis", "K", "PAYEMS", "chg"),
    ("Ventes au détail", "États-Unis", "%", "RSAFS", "pc1"),
    (
        "Confiance Conso (Mich.)",
        "États-Unis",
        "",
        "UMCSENT",
        "lin",
    ),
    // Euro area / Germany — HICP + GDP use live replacements for dead source IDs.
    (
        "HICP Zone Euro",
        "Zone euro",
        "%",
        "CP0000EZ19M086NEST",
        "pc1",
    ),
    (
        "PIB Zone Euro",
        "Zone euro",
        "%",
        "CLVMNACSCAB1GQEA19",
        "pc1",
    ),
    ("Taux Dépôt BCE", "Zone euro", "%", "ECBDFR", "lin"),
    ("IPC Allemagne", "Allemagne", "%", "CPALTT01DEM659N", "pc1"),
    (
        "Chômage Allemagne",
        "Allemagne",
        "%",
        "LRHUTTTTDEM156S",
        "lin",
    ),
    // United Kingdom
    ("IPC UK", "Royaume-Uni", "%", "CPALTT01GBM659N", "pc1"),
    ("Chômage UK", "Royaume-Uni", "%", "LRHUTTTTGBM156S", "lin"),
    // Japan
    ("Chômage Japon", "Japon", "%", "LRHUTTTTJPM156S", "lin"),
];

/// Fetch monthly US macro series from FRED. Empty if no API key set.
async fn fetch_fred(client: &reqwest::Client) -> Vec<EconIndicator> {
    let key = match std::env::var("FRED_API_KEY") {
        Ok(k) if !k.trim().is_empty() => k,
        _ => return Vec::new(),
    };

    let mut out: Vec<EconIndicator> = Vec::new();
    for (label, region, unit, series, units) in FRED_SERIES {
        let url = format!(
            "https://api.stlouisfed.org/fred/series/observations?series_id={series}&api_key={key}&file_type=json&units={units}&sort_order=desc&limit=24"
        );
        let v: serde_json::Value = match client.get(&url).send().await {
            Ok(resp) => match resp.json().await {
                Ok(j) => j,
                Err(e) => {
                    tracing::warn!(%series, error = %e, "FRED: JSON illisible");
                    continue;
                }
            },
            Err(e) => {
                tracing::warn!(%series, error = %e, "FRED: requête échouée");
                continue;
            }
        };

        let obs = match v.get("observations").and_then(|o| o.as_array()) {
            Some(o) => o,
            None => {
                tracing::warn!(%series, "FRED: pas d'observations (clé invalide ?)");
                continue;
            }
        };

        // Observations are newest-first (sort_order=desc); value "." means missing.
        let mut pairs: Vec<(String, f64)> = Vec::new();
        for o in obs {
            let date = o
                .get("date")
                .and_then(|d| d.as_str())
                .unwrap_or_default()
                .to_string();
            let val = o
                .get("value")
                .and_then(|x| x.as_str())
                .and_then(|s| s.parse::<f64>().ok());
            if let Some(v) = val {
                pairs.push((date, v));
            }
        }
        if pairs.is_empty() {
            continue;
        }

        let (year, value) = pairs[0].clone();
        let previous = pairs.get(1).map(|(_, v)| *v);
        let history: Vec<f64> = pairs.iter().rev().map(|(_, v)| *v).collect();

        out.push(EconIndicator {
            label: label.to_string(),
            region: region.to_string(),
            unit: unit.to_string(),
            year,
            value,
            previous,
            history,
            category: "macro_monthly".to_string(),
        });
    }

    tracing::info!(count = out.len(), "FRED: séries récupérées");
    out
}

/// `GET /macro/economy` — key macro indicators (cached, refreshed after TTL).
pub async fn economy() -> AppResult<Json<Vec<EconIndicator>>> {
    if let Ok(guard) = ECON_CACHE.lock() {
        if let Some((ts, data)) = guard.as_ref() {
            if ts.elapsed() < ECON_TTL {
                return Ok(Json(data.clone()));
            }
        }
    }

    let client = http_client()?;
    let mut data = fetch_economy(&client).await;
    data.extend(fetch_markets(&client).await);
    data.extend(fetch_fred(&client).await);
    if !data.is_empty() {
        if let Ok(mut guard) = ECON_CACHE.lock() {
            *guard = Some((Instant::now(), data.clone()));
        }
        return Ok(Json(data));
    }

    if let Ok(guard) = ECON_CACHE.lock() {
        if let Some((_, stale)) = guard.as_ref() {
            return Ok(Json(stale.clone()));
        }
    }
    Ok(Json(data))
}

// CFTC Commitments of Traders — TFF (Traders in Financial Futures), Futures Only.
// Open Socrata API, no key. NB: the source list named `6dca-aqww`, but that is the
// *Legacy* dataset (no leveraged-fund fields); `gpe5-46if` is the TFF dataset that
// carries `lev_money_positions_*`. Covers US futures only — no DAX (Eurex).
const COT_URL: &str = "https://publicreporting.cftc.gov/resource/gpe5-46if.json";

// (CFTC contract market code, label). The Yen code is 097741 in the TFF dataset
// (the source list's 098741 is the Legacy code and returns nothing here).
const COT_CONTRACTS: &[(&str, &str)] = &[
    ("098662", "S&P 500"),
    ("209742", "Nasdaq 100"),
    ("124603", "Dow (YM)"),
    ("099741", "EUR"),
    ("096742", "GBP"),
    ("097741", "JPY"),
];

/// Read a Socrata numeric field that may arrive as a JSON string or number.
fn socrata_num(row: &serde_json::Value, key: &str) -> Option<i64> {
    let v = row.get(key)?;
    if let Some(s) = v.as_str() {
        // Values look like "111379"; tolerate a stray decimal ("111379.0").
        return s
            .parse::<i64>()
            .ok()
            .or_else(|| s.parse::<f64>().ok().map(|f| f as i64));
    }
    v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))
}

/// Fetch the latest CoT publication for each tracked contract (leveraged funds).
async fn fetch_cot(client: &reqwest::Client) -> Vec<CotEntry> {
    let where_clause = format!(
        "cftc_contract_market_code in({})",
        COT_CONTRACTS
            .iter()
            .map(|(c, _)| format!("'{c}'"))
            .collect::<Vec<_>>()
            .join(",")
    );

    // 60 rows (10 weeks × 6 contracts) is plenty to include every contract's most
    // recent report; we pick the latest row per code below.
    let value: serde_json::Value = match client
        .get(COT_URL)
        .query(&[
            ("$where", where_clause.as_str()),
            ("$order", "report_date_as_yyyy_mm_dd DESC"),
            ("$limit", "60"),
        ])
        .send()
        .await
    {
        Ok(resp) => match resp.json().await {
            Ok(j) => j,
            Err(e) => {
                tracing::warn!(error = %e, "CoT: JSON illisible");
                return Vec::new();
            }
        },
        Err(e) => {
            tracing::warn!(error = %e, "CoT: requête échouée");
            return Vec::new();
        }
    };

    let rows = match value.as_array() {
        Some(r) => r,
        None => return Vec::new(),
    };

    let out = map_cot_rows(rows);
    tracing::info!(count = out.len(), "CoT: contrats récupérés");
    out
}

/// Pure mapping: given date-desc Socrata rows, take the latest publication of each
/// tracked contract and build its `CotEntry`. Skips contracts absent or missing
/// position fields.
fn map_cot_rows(rows: &[serde_json::Value]) -> Vec<CotEntry> {
    let mut out: Vec<CotEntry> = Vec::new();
    for (code, label) in COT_CONTRACTS {
        // Rows are date-desc, so the first match is the latest publication.
        let row = match rows
            .iter()
            .find(|r| r.get("cftc_contract_market_code").and_then(|v| v.as_str()) == Some(*code))
        {
            Some(r) => r,
            None => {
                tracing::warn!(%code, %label, "CoT: aucun rapport pour ce contrat");
                continue;
            }
        };

        let (long, short) = match (
            socrata_num(row, "lev_money_positions_long"),
            socrata_num(row, "lev_money_positions_short"),
        ) {
            (Some(l), Some(s)) => (l, s),
            _ => {
                tracing::warn!(%code, %label, "CoT: positions manquantes");
                continue;
            }
        };
        let chg = socrata_num(row, "change_in_lev_money_long").unwrap_or(0)
            - socrata_num(row, "change_in_lev_money_short").unwrap_or(0);
        let date = row
            .get("report_date_as_yyyy_mm_dd")
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(10).collect::<String>())
            .unwrap_or_default();

        out.push(CotEntry {
            marche: label.to_string(),
            net: long - short,
            chg_hebdo: chg,
            date,
        });
    }
    out
}

/// `GET /macro/cot` — weekly leveraged-fund positioning (cached, stale on failure).
pub async fn cot() -> AppResult<Json<Vec<CotEntry>>> {
    if let Ok(guard) = COT_CACHE.lock() {
        if let Some((ts, data)) = guard.as_ref() {
            if ts.elapsed() < COT_TTL {
                return Ok(Json(data.clone()));
            }
        }
    }

    let client = http_client()?;
    let items = fetch_cot(&client).await;
    if !items.is_empty() {
        if let Ok(mut guard) = COT_CACHE.lock() {
            *guard = Some((Instant::now(), items.clone()));
        }
        return Ok(Json(items));
    }

    if let Ok(guard) = COT_CACHE.lock() {
        if let Some((_, stale)) = guard.as_ref() {
            return Ok(Json(stale.clone()));
        }
    }
    Ok(Json(items))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_latest_cot_row_per_contract() {
        // Two weeks for S&P (date-desc), one for EUR, none for the rest. Fields
        // arrive as strings (as Socrata sends them), incl. a negative change.
        let rows = vec![
            json!({
                "cftc_contract_market_code": "098662",
                "report_date_as_yyyy_mm_dd": "2026-07-14T00:00:00.000",
                "lev_money_positions_long": "10000",
                "lev_money_positions_short": "14866",
                "change_in_lev_money_long": "100",
                "change_in_lev_money_short": "512"
            }),
            json!({
                "cftc_contract_market_code": "098662",
                "report_date_as_yyyy_mm_dd": "2026-07-07T00:00:00.000",
                "lev_money_positions_long": "9000",
                "lev_money_positions_short": "9000",
                "change_in_lev_money_long": "0",
                "change_in_lev_money_short": "0"
            }),
            json!({
                "cftc_contract_market_code": "099741",
                "report_date_as_yyyy_mm_dd": "2026-07-14T00:00:00.000",
                "lev_money_positions_long": "80000",
                "lev_money_positions_short": "133691",
                "change_in_lev_money_long": "1000",
                "change_in_lev_money_short": "9230"
            }),
        ];

        let out = map_cot_rows(&rows);
        // Only the two contracts present in the data are returned.
        assert_eq!(out.len(), 2);

        let sp = &out[0]; // COT_CONTRACTS order: S&P 500 first
        assert_eq!(sp.marche, "S&P 500");
        assert_eq!(sp.net, 10000 - 14866); // latest week only
        assert_eq!(sp.chg_hebdo, 100 - 512); // -412
        assert_eq!(sp.date, "2026-07-14");

        let eur = &out[1];
        assert_eq!(eur.marche, "EUR");
        assert_eq!(eur.net, 80000 - 133691);
        assert_eq!(eur.chg_hebdo, 1000 - 9230);
    }

    #[test]
    fn socrata_num_parses_string_number_and_negative() {
        let row = json!({ "s": "111379", "n": 42, "neg": "-412", "dec": "100.0" });
        assert_eq!(socrata_num(&row, "s"), Some(111379));
        assert_eq!(socrata_num(&row, "n"), Some(42));
        assert_eq!(socrata_num(&row, "neg"), Some(-412));
        assert_eq!(socrata_num(&row, "dec"), Some(100));
        assert_eq!(socrata_num(&row, "missing"), None);
    }
}
