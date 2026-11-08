-- Link journal accounts to the MT5 terminals that feed them, and tag trades
-- with the plan that produced them.
--
-- Every column is nullable on purpose. The journal predates the MT5 bridge:
-- accounts created by hand, trades imported from a CSV export and the Scale FX
-- account have no terminal behind them and must stay perfectly valid.

-- Which MT5 terminal feeds this account ---------------------------------------
--
-- `mt5_login` is the account number shown by the terminal (e.g. 100000001) and
-- `mt5_server` the broker server it connects to (e.g. "BrokerDemo-MT5 1").
-- Together they are what `accounts.toml` keys a portable terminal on, and what
-- `POST /ingest/trades` uses to decide which journal account a batch belongs
-- to. The sender never names an account id: it states the login it is logged
-- into, and the server resolves it. A machine credential therefore cannot
-- write into an account it is not actually connected to.
ALTER TABLE accounts ADD COLUMN mt5_login  TEXT;
ALTER TABLE accounts ADD COLUMN mt5_server TEXT;

-- One journal account per MT5 login. Without this, two accounts could claim
-- the same login and the resolution above would silently pick one of them —
-- trades would land in whichever row the query planner returned first, and the
-- mistake would only surface as drifting P&L weeks later.
--
-- Partial index: the uniqueness applies to real logins only, so the many
-- accounts with no terminal (manual, CSV, Scale FX) all keep a NULL and do not
-- collide with each other.
CREATE UNIQUE INDEX IF NOT EXISTS idx_accounts_mt5_login
    ON accounts(mt5_login) WHERE mt5_login IS NOT NULL;

-- Which trade plan produced this trade ----------------------------------------
--
-- A plan places several positions at once — thirds sharing one stop with
-- staggered targets — and each becomes its own broker ticket, hence its own
-- row here. `plan_id` is what stitches them back into the single line the
-- journal shows. NULL means a trade taken by hand or imported, which is the
-- case for everything recorded so far.
ALTER TABLE trades ADD COLUMN plan_id TEXT;

CREATE INDEX IF NOT EXISTS idx_trades_plan
    ON trades(plan_id) WHERE plan_id IS NOT NULL;
