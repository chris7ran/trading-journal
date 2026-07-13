-- Sprint 1 initial schema.
-- Follows ARCHITECTURE.md (accounts, prop_rules, trades, sessions).
-- SQLite dialect. Foreign keys are enabled at runtime via PRAGMA in db.rs.

-- Trading accounts ----------------------------------------------------------
CREATE TABLE IF NOT EXISTS accounts (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    broker      TEXT NOT NULL DEFAULT 'FusionMarkets',
    balance     REAL,
    currency    TEXT NOT NULL DEFAULT 'USD',
    is_funded   INTEGER NOT NULL DEFAULT 1,   -- SQLite has no native BOOLEAN (0/1)
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Prop firm rules -----------------------------------------------------------
CREATE TABLE IF NOT EXISTS prop_rules (
    id                    TEXT PRIMARY KEY,
    account_id            TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    daily_drawdown_max    REAL,           -- e.g. 0.05 = 5%
    global_drawdown_max   REAL,           -- e.g. 0.10 = 10%
    profit_target         REAL,
    min_trading_days      INTEGER,
    consistency_rule_pct  REAL NOT NULL DEFAULT 0.20,
    lot_size_max          REAL
);

-- Trades --------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS trades (
    id              TEXT PRIMARY KEY,
    account_id      TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    symbol          TEXT NOT NULL,          -- e.g. GER40, EURUSD
    direction       TEXT,                   -- LONG / SHORT
    open_time       TEXT,                   -- ISO-8601 (UTC, no timezone suffix)
    close_time      TEXT,
    open_price      REAL,
    close_price     REAL,
    lot_size        REAL,
    pnl             REAL,
    pnl_pct         REAL,
    commission      REAL,
    swap            REAL,
    setup_tag       TEXT,                   -- e.g. "Break & Retest"
    emotion_tag     TEXT,                   -- e.g. "Confident", "FOMO"
    notes           TEXT,
    screenshot_url  TEXT,
    mt5_ticket      TEXT UNIQUE,            -- MT5 id, used to dedupe CSV imports
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_trades_account   ON trades(account_id);
CREATE INDEX IF NOT EXISTS idx_trades_symbol    ON trades(symbol);
CREATE INDEX IF NOT EXISTS idx_trades_open_time ON trades(open_time);

-- Trading sessions (one trading day) ----------------------------------------
CREATE TABLE IF NOT EXISTS sessions (
    id          TEXT PRIMARY KEY,
    account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    date        TEXT NOT NULL,              -- YYYY-MM-DD
    mood_pre    INTEGER,                    -- 1-5 before session
    mood_post   INTEGER,                    -- 1-5 after session
    notes       TEXT,
    ai_report   TEXT,                       -- generated AI report
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_sessions_account_date ON sessions(account_id, date);

-- Seed a default account so trade entry / CSV import work out of the box.
INSERT OR IGNORE INTO accounts (id, name, broker, currency, is_funded)
VALUES ('default', 'Default Account', 'FusionMarkets', 'USD', 1);
