-- Immutable archive of the morning brief.
--
-- # Why store something that is recomputable
--
-- Every figure in a brief derives from `candles_m5`, so in principle it could
-- always be recomputed. Two reasons it must not be:
--
--   1. **The rules will change.** Widen the ADR window, refine the bias, and
--      every past brief silently changes with it. Comparing today's decisions
--      against a retroactively rewritten brief proves nothing.
--   2. **A brief is a point in time.** The one read before the London open
--      knows the previous session and the Asian range; the same brief
--      recomputed in the evening also knows the day's range, its sweeps and
--      its close. They are different documents. Only the first says anything
--      about what was known when a decision was taken.
--
-- Hence: the payload is frozen exactly as it was served, and `captured_at`
-- records when.
CREATE TABLE IF NOT EXISTS brief_snapshots (
    id          TEXT PRIMARY KEY,
    symbol      TEXT NOT NULL,
    -- Paris trading day the brief describes, YYYY-MM-DD.
    date        TEXT NOT NULL,
    -- Which pre-session capture: 'pre_london', 'pre_ny', or 'manual'.
    -- Instruments do not open together (DAX 09:00 Paris, US indices 15:30), so
    -- a single daily capture would be six hours stale for three of the four
    -- symbols followed here.
    session     TEXT NOT NULL,
    captured_at TEXT NOT NULL,          -- UTC, ISO-8601
    -- The brief as served, verbatim. Read back for display; never recomputed.
    payload     TEXT NOT NULL,

    -- Denormalised copies of the fields worth filtering and grouping on, so
    -- the review does not have to parse every payload to answer "how many days
    -- read as a reversal?".
    bias_read          TEXT,
    open_above_mid     INTEGER,         -- 0/1, SQLite has no BOOLEAN
    previous_mid       REAL,
    adr                REAL,
    day_pct_of_adr     REAL,
    asia_pct_of_median REAL,

    -- One capture per symbol, day and session. Writes are INSERT OR IGNORE, so
    -- the first capture wins: a later call cannot overwrite the morning's
    -- snapshot with an end-of-day view of the same day.
    UNIQUE (symbol, date, session)
);

-- The review walks a date range across all symbols, which the UNIQUE index
-- (symbol first) does not serve.
CREATE INDEX IF NOT EXISTS idx_brief_snapshots_date ON brief_snapshots(date);
