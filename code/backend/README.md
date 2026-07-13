# Trading Journal API

Self-hosted backend for a personal trading journal. Rust (axum) + SQLite.
Sprint 1 scope: trades CRUD, MT5 CSV import, JWT auth, Docker + systemd deploy.

## Quick start (local dev)

```bash
cd backend
cp .env.example .env

# 1. Generate secrets
echo "JWT_SECRET=$(openssl rand -hex 32)" >> .env       # or edit .env by hand
cargo run --release -- hash 'my-password'               # paste output into ADMIN_PASSWORD_HASH

# 2. Run
cargo run                       # listens on 0.0.0.0:8080, creates ./journal.db
cargo test                      # run unit + integration tests
```

### Seed demo data (optional)

Populate the database with ~10 demo trades so the app isn't empty on first run.
Idempotent (safe to re-run): trades are keyed by `mt5_ticket`.

```bash
PASSWORD='your-password' ./scripts/seed.sh
# or against a remote server:
BASE_URL=http://100.x.y.z:8080 PASSWORD='your-password' ./scripts/seed.sh
```

## API

| Method | Path                  | Auth | Description                          |
|--------|-----------------------|------|--------------------------------------|
| GET    | `/health`             | no   | Liveness + DB check                  |
| POST   | `/auth/login`         | no   | `{ "password": "..." }` → JWT        |
| POST   | `/auth/verify-token`  | yes  | Validate the bearer token            |
| GET    | `/trades`             | yes  | List + filters (see below)           |
| POST   | `/trades`             | yes  | Create a trade                       |
| GET    | `/trades/:id`         | yes  | Get one trade                        |
| PUT    | `/trades/:id`         | yes  | Partial update (notes, tags, ...)    |
| GET    | `/trades/stats`       | yes  | Aggregated analytics (+ filters)     |
| POST   | `/trades/import/csv`  | yes  | Bulk import MT5 CSV (dedupes ticket) |
| GET    | `/accounts`           | yes  | List trading accounts                |
| POST   | `/accounts`           | yes  | Create an account                    |
| PUT    | `/accounts/:id`       | yes  | Update account (name, balance, ...)  |
| GET    | `/accounts/:id/rules` | yes  | Get prop firm rules (404 if unset)   |
| PUT    | `/accounts/:id/rules` | yes  | Insert-or-update prop firm rules     |
| GET    | `/setups`             | yes  | List trading setups                  |
| POST   | `/setups`             | yes  | Create a setup (name + rules)        |
| GET    | `/macro/calendar`     | yes  | Economic calendar (red/orange)       |
| GET    | `/macro/news`         | yes  | Aggregated news + sentiment (RSS)    |
| GET    | `/macro/economy`      | yes  | Macro indicators (World Bank)        |

Auth: send `Authorization: Bearer <token>` on protected routes.

`GET /trades` filters (query params): `account_id`, `symbol`, `direction`,
`from`, `to` (ISO-8601 on open_time), `limit` (1–1000, default 100), `offset`.

`POST /trades/import/csv`: body is the raw CSV (`Content-Type: text/csv`),
optional `?account_id=...`. Response: `{ imported, skipped_duplicates, failed, warnings }`.

MT5 import: the endpoint accepts the MT5 history report **directly as `.xlsx`**
(the format MT5 exports) **or** as CSV — it auto-detects by magic bytes (`.xlsx`
is a ZIP starting with `PK\x03\x04`). XLSX is read with `calamine`; no manual
CSV conversion needed. Both paths share the same parser, which is
**section-aware** (extracts the `Positions` block),
**bilingual** (FR `Symbole/Heure/Prix/Echange` and EN `Symbol/Time/Price/Swap`),
and **auto-detects the delimiter** (`,`, `;` or tab — French locale exports use
`;` with comma decimals). It was calibrated against a real GoatFunded export.
Dedup is by `mt5_ticket` (the MT5 `Position` id). See `src/csv_import/mt5.rs`.

`GET /trades/stats` filters (query params): `account_id`, `symbol`, `from`, `to`.
Response: `{ total_trades, wins, losses, win_rate, total_pnl, avg_win, avg_loss,
profit_factor, gross_profit, gross_loss, by_symbol: [{ symbol, trades, wins, pnl }] }`.
A win is `pnl > 0`, a loss is `pnl < 0`; break-even trades count only in `total_trades`.

## Layout

```
src/
  main.rs           binary entry point (+ `hash` subcommand)
  lib.rs            library crate (so tests can drive the router)
  config.rs         env-based configuration
  db.rs             SQLite pool + migrations
  state.rs          shared AppState
  error.rs          AppError -> HTTP responses
  models.rs         Trade + request/response DTOs
  auth/             jwt, password (Argon2), login handler, auth middleware
  routes/           health, trades (CRUD), import (CSV)
  csv_import/mt5.rs tolerant MT5 CSV parser (+ unit tests)
migrations/         0001_init.sql (accounts, prop_rules, trades, sessions)
tests/api.rs        end-to-end integration tests
deploy/             systemd unit + Proxmox/Tailscale deployment guide
Dockerfile          multi-stage build -> slim runtime image
```

## Notes / deviations from ARCHITECTURE.md

- `tower-http` is pinned to **0.5** (not 0.4): axum 0.7 uses the http 1.0
  ecosystem and is incompatible with tower-http 0.4.
- Auth uses a **password → JWT** login (`POST /auth/login`) rather than only
  `verify-token`; this is the standard single-user pattern and is extensible.
- Timestamps are stored as ISO-8601 **TEXT** in SQLite for now.
- The MT5 CSV parser is **tolerant** but should be calibrated against a real
  FusionMarkets export (see the TODO in `src/csv_import/mt5.rs`).
