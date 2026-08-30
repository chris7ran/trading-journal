-- Raw M5 candles pushed by the MT5 Expert Advisor.
--
-- Design notes:
--   * `ts` is the candle OPEN time, ISO-8601 in **UTC** ("2026-08-30T07:00:00Z").
--     MT5 reports server time, so the EA converts with TimeGMT() before sending.
--     Storing UTC keeps session maths timezone-safe (London/NY/Tokyo DST differ).
--   * The composite primary key makes re-sending a candle idempotent: the EA can
--     replay a day without creating duplicates (see the UPSERT in routes/ingest.rs).
--   * WITHOUT ROWID: the table is a pure key/value lookup by (symbol, ts), so the
--     extra rowid B-tree would only cost space. ~66k rows for 60 days x 4 symbols.
CREATE TABLE IF NOT EXISTS candles_m5 (
    symbol  TEXT NOT NULL,
    ts      TEXT NOT NULL,          -- candle open time, UTC, ISO-8601
    open    REAL NOT NULL,
    high    REAL NOT NULL,
    low     REAL NOT NULL,
    close   REAL NOT NULL,
    volume  REAL,
    PRIMARY KEY (symbol, ts)
) WITHOUT ROWID;

-- No secondary index on purpose. Every query is either "one symbol over a time
-- window" or "GROUP BY symbol", and the composite primary key serves both. An
-- index on `ts` alone would only add write amplification on each ingested batch.
