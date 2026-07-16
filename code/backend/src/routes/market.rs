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
use crate::models::{EcoEvent, EconIndicator, NewsItem};

// In-memory caches. faireconomy rate-limits to 2 downloads / 5 min, so we serve
// a cached copy and only refresh after the TTL — and fall back to stale data if
// a live refresh is rate-limited or fails.
const CALENDAR_TTL: Duration = Duration::from_secs(15 * 60);
const NEWS_TTL: Duration = Duration::from_secs(10 * 60);
const ECON_TTL: Duration = Duration::from_secs(12 * 60 * 60); // annual data — refresh rarely

static CALENDAR_CACHE: Mutex<Option<(Instant, Vec<EcoEvent>)>> = Mutex::new(None);
static NEWS_CACHE: Mutex<Option<(Instant, Vec<NewsItem>)>> = Mutex::new(None);
static ECON_CACHE: Mutex<Option<(Instant, Vec<EconIndicator>)>> = Mutex::new(None);

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
// indicators, news_25=stock/indices. MarketWatch = US/Wall Street.
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

// FRED monthly US series (needs a free API key in FRED_API_KEY):
// (label, region, unit, series_id, units). units "pc1" = % change vs year ago.
const FRED_SERIES: &[(&str, &str, &str, &str, &str)] = &[
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
