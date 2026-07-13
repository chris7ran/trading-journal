-- Sprint 11: trading setups + per-trade review fields.

-- Named setups / patterns (with their own rules text).
CREATE TABLE IF NOT EXISTS setups (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    rules       TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Post-trade review (1 = yes, 0 = no, NULL = not reviewed).
-- The chosen setup name is stored in the existing trades.setup_tag column.
ALTER TABLE trades ADD COLUMN followed_plan  INTEGER;
ALTER TABLE trades ADD COLUMN respected_sl   INTEGER;
ALTER TABLE trades ADD COLUMN pattern_valid  INTEGER;
ALTER TABLE trades ADD COLUMN thesis_worked  INTEGER;
ALTER TABLE trades ADD COLUMN good_exit      INTEGER;
