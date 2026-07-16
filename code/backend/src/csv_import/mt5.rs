//! Parser for MetaTrader 5 history exports (the "Rapport d'historique" / "Trade
//! History Report"), saved as CSV.
//!
//! ## Real-world shape this handles
//! The MT5 report is a **multi-section** document, not a single-header CSV:
//!
//! ```text
//! Rapport d'historique de trading
//! Nom:,,,trader-...
//! Compte:,,,"XXXXXXXXX (USD, GoatFunded-Server, real, Hedge)"
//! ...
//! Positions
//! Heure,Position,Symbole,Type,Volume,Prix,S / L,T / P,Heure,Prix,Commission,Echange,Profit
//! 2025.12.23 12:27:24,42047402,US30.x,buy,2,48380.15,...,2025.12.24 16:34:47,48445,0.0,-8.06,128.9
//! ...
//! Ordres
//! ...
//! Transactions
//! ...
//! ```
//!
//! We extract the **Positions** section (one row per closed position, with open
//! + close data and P&L) and stop at the next section.
//!
//! ## Robustness
//! - **Bilingual** column names (FR/EN): Symbole/Symbol, Heure/Time, Prix/Price,
//!   Echange/Swap, Position/Ticket, Profit, etc.
//! - **Delimiter auto-detection** (`,`, `;` or tab): French locale exports from
//!   Excel/Numbers often use `;` with comma decimals.
//! - **Duplicate `Heure`/`Prix` columns**: first = open, second = close.
//! - Calibrated and validated against a real GoatFunded MT5 export.

use std::io::{Cursor, Read, Write};

use calamine::{open_workbook_from_rs, Data, Reader, Xlsx};
use csv::{ReaderBuilder, Trim};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::models::NewTrade;

/// Result of parsing: the trades we understood, plus warnings.
#[derive(Debug, Default)]
pub struct ParseResult {
    pub trades: Vec<NewTrade>,
    pub warnings: Vec<String>,
}

/// Section titles that mark the end of the Positions block (lowercased).
const SECTION_KEYWORDS: &[&str] = &[
    "positions",
    "position",
    "ordres",
    "orders",
    "transactions",
    "deals",
    "résumé",
    "resume",
    "summary",
];

/// Parse an MT5 history report from raw **CSV** text.
pub fn parse(csv_text: &str, account_id: &str) -> Result<ParseResult, String> {
    let delimiter = sniff_delimiter(csv_text);

    let mut reader = ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(Trim::All)
        .delimiter(delimiter)
        .from_reader(csv_text.as_bytes());

    // Materialize all rows so we can scan for the Positions section.
    let mut rows: Vec<Vec<String>> = Vec::new();
    for record in reader.records() {
        match record {
            Ok(r) => rows.push(r.iter().map(|s| s.trim().to_string()).collect()),
            Err(_) => continue, // skip malformed lines rather than abort
        }
    }
    parse_rows(rows, account_id)
}

/// Parse an MT5 history report directly from an **.xlsx** file (the format MT5
/// exports). Avoids the manual XLSX -> CSV conversion (and its locale pitfalls).
pub fn parse_xlsx(bytes: &[u8], account_id: &str) -> Result<ParseResult, String> {
    // MT5 writes the XLSX's internal XML as UTF-16, which calamine/quick-xml
    // cannot read ("Unexpected end of xml"). Re-pack the archive with its XML
    // entries transcoded to UTF-8 before handing it to calamine.
    let normalized =
        normalize_xlsx_encoding(bytes).map_err(|e| format!("préparation XLSX impossible: {e}"))?;

    let cursor = Cursor::new(normalized);
    let mut workbook: Xlsx<_> =
        open_workbook_from_rs(cursor).map_err(|e| format!("XLSX illisible: {e}"))?;

    let range = workbook
        .worksheet_range_at(0)
        .ok_or_else(|| "aucune feuille dans le fichier XLSX".to_string())?
        .map_err(|e| format!("feuille XLSX illisible: {e}"))?;

    let rows: Vec<Vec<String>> = range
        .rows()
        .map(|row| row.iter().map(cell_to_string).collect())
        .collect();

    parse_rows(rows, account_id)
}

/// Shared parsing core: takes a grid of string cells (from CSV or XLSX) and
/// extracts the Positions section into trades.
fn parse_rows(rows: Vec<Vec<String>>, account_id: &str) -> Result<ParseResult, String> {
    if rows.is_empty() {
        return Err("fichier vide".to_string());
    }

    let header_idx = locate_positions_header(&rows)
        .ok_or_else(|| "section 'Positions' introuvable dans le rapport MT5".to_string())?;

    let header: Vec<String> = rows[header_idx].iter().map(|s| s.to_lowercase()).collect();
    let cols = ColumnMap::from_headers(&header);
    if cols.symbol.is_none() {
        return Err("colonne 'Symbole' introuvable dans la section Positions".to_string());
    }

    let mut result = ParseResult::default();

    for raw in rows.iter().skip(header_idx + 1) {
        // Stop at a blank row or the next section.
        if raw.iter().all(|c| c.is_empty()) {
            break;
        }
        let first = raw.first().map(|s| s.to_lowercase()).unwrap_or_default();
        if SECTION_KEYWORDS.contains(&first.as_str()) {
            break;
        }

        let get = |i: Option<usize>| -> Option<String> {
            i.and_then(|idx| raw.get(idx))
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        };

        let symbol = match get(cols.symbol) {
            Some(s) => s,
            None => continue, // not a trade row (summary/blank)
        };

        let trade = NewTrade {
            account_id: Some(account_id.to_string()),
            symbol,
            direction: get(cols.type_).map(|t| normalize_direction(&t)),
            open_time: get(cols.open_time).map(normalize_datetime),
            close_time: get(cols.close_time).map(normalize_datetime),
            open_price: get(cols.open_price).and_then(|v| parse_number(&v)),
            close_price: get(cols.close_price).and_then(|v| parse_number(&v)),
            lot_size: get(cols.volume).and_then(|v| parse_number(&v)),
            pnl: get(cols.profit).and_then(|v| parse_number(&v)),
            pnl_pct: None,
            commission: get(cols.commission).and_then(|v| parse_number(&v)),
            swap: get(cols.swap).and_then(|v| parse_number(&v)),
            setup_tag: None,
            emotion_tag: None,
            notes: None,
            screenshot_url: None,
            mt5_ticket: get(cols.ticket),
            // Imported trades have no post-trade review yet.
            followed_plan: None,
            respected_sl: None,
            pattern_valid: None,
            thesis_worked: None,
            good_exit: None,
        };

        result.trades.push(trade);
    }

    if result.trades.is_empty() {
        result
            .warnings
            .push("Aucune position trouvée dans la section Positions.".to_string());
    }

    Ok(result)
}

/// Find the index of the Positions header row.
/// Primary: the row following a `Positions` section title. Fallback: the first
/// row that looks like a header (has a symbol column and a type column).
fn locate_positions_header(rows: &[Vec<String>]) -> Option<usize> {
    // Primary: a title row whose first cell is exactly "positions".
    for (i, row) in rows.iter().enumerate() {
        if row.first().map(|s| s.trim().to_lowercase()).as_deref() == Some("positions") {
            // Header is the next non-empty row.
            for (j, candidate) in rows.iter().enumerate().skip(i + 1) {
                if candidate.iter().any(|c| !c.is_empty()) {
                    return Some(j);
                }
            }
        }
    }

    // Fallback: first header-looking row.
    for (i, row) in rows.iter().enumerate() {
        let low: Vec<String> = row.iter().map(|s| s.to_lowercase()).collect();
        let has_symbol = low.iter().any(|c| c == "symbole" || c == "symbol");
        let has_type = low.iter().any(|c| c == "type" || c == "direction");
        if has_symbol && has_type {
            return Some(i);
        }
    }

    None
}

/// Resolved column indices for the fields we care about.
#[derive(Debug, Default)]
struct ColumnMap {
    ticket: Option<usize>,
    symbol: Option<usize>,
    type_: Option<usize>,
    volume: Option<usize>,
    open_time: Option<usize>,
    close_time: Option<usize>,
    open_price: Option<usize>,
    close_price: Option<usize>,
    commission: Option<usize>,
    swap: Option<usize>,
    profit: Option<usize>,
}

impl ColumnMap {
    /// `header` must already be lowercased.
    fn from_headers(header: &[String]) -> Self {
        let find = |names: &[&str]| -> Option<usize> {
            header.iter().position(|h| names.contains(&h.as_str()))
        };
        // Indices of all columns whose header *contains* any needle — used for
        // the duplicated Heure/Prix (Time/Price) columns.
        let find_all = |needles: &[&str]| -> Vec<usize> {
            header
                .iter()
                .enumerate()
                .filter(|(_, h)| needles.iter().any(|n| h.contains(n)))
                .map(|(i, _)| i)
                .collect()
        };

        let times = find_all(&["heure", "time"]);
        let prices = find_all(&["prix", "price"]);

        let open_time =
            find(&["heure d'ouverture", "open time"]).or_else(|| times.first().copied());
        let close_time = find(&["heure de clôture", "heure de cloture", "close time"])
            .or_else(|| times.get(1).copied());
        let open_price =
            find(&["prix d'ouverture", "open price"]).or_else(|| prices.first().copied());
        let close_price = find(&["prix de clôture", "prix de cloture", "close price"])
            .or_else(|| prices.get(1).copied());

        Self {
            ticket: find(&["position", "ticket", "deal", "ordre", "order"]),
            symbol: find(&["symbole", "symbol"]),
            type_: find(&["type", "direction"]),
            volume: find(&["volume", "lots", "lot", "size", "taille"]),
            open_time,
            close_time,
            open_price,
            close_price,
            commission: find(&["commission"]),
            swap: find(&["echange", "échange", "swap"]),
            profit: find(&["profit", "p/l", "pnl", "benefice", "bénéfice"]),
        }
    }
}

/// Convert an XLSX cell to a string, formatting integer-valued floats without a
/// trailing `.0` (so a ticket `42047402.0` becomes `"42047402"`). MT5 stores its
/// timestamps as text, so those arrive as `Data::String`.
fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::String(s) => s.trim().to_string(),
        Data::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                f.to_string()
            }
        }
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTimeIso(s) => s.clone(),
        Data::DurationIso(s) => s.clone(),
        // DateTime (Excel serial), Error, Empty -> empty string.
        _ => String::new(),
    }
}

/// Re-pack an .xlsx (a ZIP of XML parts), transcoding any UTF-16 XML entry to
/// UTF-8 (and fixing its `encoding=` declaration). Non-UTF-16 entries are copied
/// as-is. Returns the new archive bytes.
fn normalize_xlsx_encoding(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut out: Vec<u8> = Vec::new();
    {
        let mut zip = ZipWriter::new(Cursor::new(&mut out));
        let options: FileOptions =
            FileOptions::default().compression_method(CompressionMethod::Stored);

        // Zip-bomb guard: cap the decompressed bytes we read, per entry and
        // cumulatively, so a small malicious .xlsx can't blow up memory.
        const MAX_ENTRY_BYTES: u64 = 25 * 1024 * 1024; // 25 MiB per entry
        const MAX_TOTAL_BYTES: u64 = 100 * 1024 * 1024; // 100 MiB total
        let mut total_decompressed: u64 = 0;

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
            let name = entry.name().to_string();

            if name.ends_with('/') {
                zip.add_directory(name, options)
                    .map_err(|e| e.to_string())?;
                continue;
            }

            // Read at most `cap + 1` bytes; if we hit that, the entry is over
            // budget and we reject the whole file.
            let cap = MAX_TOTAL_BYTES
                .saturating_sub(total_decompressed)
                .min(MAX_ENTRY_BYTES);
            let mut content = Vec::new();
            let read = (&mut entry)
                .take(cap + 1)
                .read_to_end(&mut content)
                .map_err(|e| e.to_string())?;
            if read as u64 > cap {
                return Err(format!(
                    "xlsx entry '{name}' exceeds the decompression limit (possible zip bomb)"
                ));
            }
            total_decompressed += read as u64;
            let content = transcode_utf16(content);

            zip.start_file(name, options).map_err(|e| e.to_string())?;
            zip.write_all(&content).map_err(|e| e.to_string())?;
        }
        zip.finish().map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// If `bytes` start with a UTF-16 BOM, decode to UTF-8 and rewrite the XML
/// encoding declaration. Otherwise return the bytes unchanged.
fn transcode_utf16(bytes: Vec<u8>) -> Vec<u8> {
    let decoded = if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16(&units).ok()
    } else if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16(&units).ok()
    } else {
        None
    };

    match decoded {
        Some(s) => s
            .replace("UTF-16", "UTF-8")
            .replace("utf-16", "UTF-8")
            .into_bytes(),
        None => bytes,
    }
}

/// Detect the field delimiter from the first lines (`,`, `;` or tab).
fn sniff_delimiter(text: &str) -> u8 {
    let head: String = text.lines().take(30).collect::<Vec<_>>().join("\n");
    let commas = head.matches(',').count();
    let semis = head.matches(';').count();
    let tabs = head.matches('\t').count();

    if semis > commas && semis >= tabs {
        b';'
    } else if tabs > commas && tabs > semis {
        b'\t'
    } else {
        b','
    }
}

/// MT5 uses "buy"/"sell"; we store LONG/SHORT.
fn normalize_direction(raw: &str) -> String {
    let lower = raw.to_lowercase();
    if lower.contains("buy") {
        "LONG".to_string()
    } else if lower.contains("sell") {
        "SHORT".to_string()
    } else {
        raw.to_uppercase()
    }
}

/// Normalize MT5 timestamps (`2026.06.20 13:45:00`) to ISO-8601
/// (`2026-06-20T13:45:00`). Leaves unrecognized formats untouched.
fn normalize_datetime(raw: String) -> String {
    let replaced = raw.replacen('.', "-", 2); // only the date part uses dots
    if let Some((date, time)) = replaced.split_once(' ') {
        format!("{date}T{time}")
    } else {
        replaced
    }
}

/// Parse a numeric cell, tolerating spaces and comma decimals.
fn parse_number(raw: &str) -> Option<f64> {
    let cleaned: String = raw
        .replace([' ', '\u{a0}'], "") // space + non-breaking space
        .replace(',', ".");
    cleaned.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // A trimmed slice of a real GoatFunded MT5 report (French, comma-delimited),
    // including the title/metadata rows, the Positions section, and the start of
    // the next section to verify we stop correctly.
    const REAL_FR: &str = "\
Rapport d'historique de trading
Nom:,,,trader-[Live_Account]_INSTANT_PRO
Compte:,,,\"XXXXXXXXX (USD, GoatFunded-Server, real, Hedge)\"
Courtier:,,,Goat Funded Ltd.
Date:,,,2026.06.28 21:18
Positions
Heure,Position,Symbole,Type,Volume,Prix,S / L,T / P,Heure,Prix,Commission,Echange,Profit
2025.12.23 12:27:24,42047402,US30.x,buy,2,48380.15,48108,48639,2025.12.24 16:34:47,48445,0.0,-8.06,128.9
2026.02.04 17:14:35,54200335,US30.x,sell,2,49496.55,49673,49226,2026.02.04 19:50:14,49224,0.0,0.0,545.0
2026.06.15 08:02:41,108534700,GER40.x,sell,3,25042.58,25190.3,24887.8,2026.06.15 18:15:09,24887.7,0.0,0.0,539.12
Ordres
Heure d'ouverture,Ordre,Symbole,Type,Volume,Prix,S / L,T / P,Heure,État,,Commentaire
2025.12.23 12:27:24,42047402,US30.x,buy,2 / 2,market,,,2025.12.23 12:27:24,filled";

    #[test]
    fn parses_real_french_report_positions_only() {
        let res = parse(REAL_FR, "acct-1").expect("parse ok");
        // 3 positions; the Ordres section must NOT be parsed.
        assert_eq!(res.trades.len(), 3);

        let t0 = &res.trades[0];
        assert_eq!(t0.symbol, "US30.x");
        assert_eq!(t0.direction.as_deref(), Some("LONG")); // buy
        assert_eq!(t0.mt5_ticket.as_deref(), Some("42047402"));
        assert_eq!(t0.open_time.as_deref(), Some("2025-12-23T12:27:24"));
        assert_eq!(t0.close_time.as_deref(), Some("2025-12-24T16:34:47"));
        assert_eq!(t0.open_price, Some(48380.15));
        assert_eq!(t0.close_price, Some(48445.0));
        assert_eq!(t0.lot_size, Some(2.0));
        assert_eq!(t0.pnl, Some(128.9));
        assert_eq!(t0.swap, Some(-8.06));
        assert_eq!(t0.account_id.as_deref(), Some("acct-1"));

        assert_eq!(res.trades[1].direction.as_deref(), Some("SHORT")); // sell
        assert_eq!(res.trades[2].symbol, "GER40.x");
    }

    #[test]
    fn detects_semicolon_delimiter_and_comma_decimals() {
        // Same Positions block, French-locale style: ';' delimiter, ',' decimals.
        let csv = "\
Positions
Heure;Position;Symbole;Type;Volume;Prix;S / L;T / P;Heure;Prix;Commission;Echange;Profit
2025.12.23 12:27:24;42047402;US30.x;buy;2;48380,15;48108;48639;2025.12.24 16:34:47;48445;0,0;-8,06;128,9";
        let res = parse(csv, "acct-1").expect("parse ok");
        assert_eq!(res.trades.len(), 1);
        let t = &res.trades[0];
        assert_eq!(t.open_price, Some(48380.15));
        assert_eq!(t.pnl, Some(128.9));
        assert_eq!(t.swap, Some(-8.06));
    }

    #[test]
    fn english_single_section_fallback() {
        // No "Positions" title; falls back to the header-looking row.
        let csv = "\
Time,Position,Symbol,Type,Volume,Price,S/L,T/P,Time,Price,Commission,Swap,Profit
2026.06.20 09:30:00,123,EURUSD,Buy,0.10,1.0850,0,0,2026.06.20 11:00:00,1.0900,-0.5,0,50.0";
        let res = parse(csv, "acct-1").expect("parse ok");
        assert_eq!(res.trades.len(), 1);
        assert_eq!(res.trades[0].close_price, Some(1.0900));
        assert_eq!(res.trades[0].direction.as_deref(), Some("LONG"));
    }

    #[test]
    fn missing_symbol_column_errors() {
        let csv = "Ticket,Type,Volume\n1,Buy,0.1";
        assert!(parse(csv, "acct-1").is_err());
    }

    #[test]
    fn parse_number_handles_locales() {
        assert_eq!(parse_number("48380.15"), Some(48380.15));
        assert_eq!(parse_number("48380,15"), Some(48380.15)); // comma decimal
        assert_eq!(parse_number("1 234,5"), Some(1234.5)); // space thousands
        assert_eq!(parse_number("\u{a0}-8,06"), Some(-8.06)); // non-breaking space + sign
        assert_eq!(parse_number(""), None);
        assert_eq!(parse_number("n/a"), None);
    }

    #[test]
    fn normalize_direction_maps_buy_sell() {
        assert_eq!(normalize_direction("buy"), "LONG");
        assert_eq!(normalize_direction("Sell"), "SHORT");
        assert_eq!(normalize_direction("balance"), "BALANCE"); // unknown -> upper
    }

    #[test]
    fn normalize_datetime_to_iso() {
        assert_eq!(
            normalize_datetime("2026.06.20 13:45:00".to_string()),
            "2026-06-20T13:45:00"
        );
        assert_eq!(normalize_datetime("2026.06.20".to_string()), "2026-06-20");
    }

    #[test]
    fn sniff_delimiter_detects_separators() {
        assert_eq!(sniff_delimiter("a;b;c\n1;2;3"), b';');
        assert_eq!(sniff_delimiter("a\tb\tc\n1\t2\t3"), b'\t');
        assert_eq!(sniff_delimiter("a,b,c\n1,2,3"), b',');
    }
}
