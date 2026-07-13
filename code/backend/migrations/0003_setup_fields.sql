-- Sprint 17: structured fields on setups (AccuTrader-style detail page).
ALTER TABLE setups ADD COLUMN description  TEXT;
ALTER TABLE setups ADD COLUMN target_entry TEXT;
ALTER TABLE setups ADD COLUMN target_exit  TEXT;
ALTER TABLE setups ADD COLUMN stop_loss    TEXT;
