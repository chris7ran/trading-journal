# MT5 candle feed

`TradingJournalFeed.mq5` is an Expert Advisor whose only job is to push closed
**M5 candles** to the journal's `/ingest/candles` endpoint. From those candles
the backend derives the daily levels: session ranges, opening ranges, previous-day
extremes and sweeps.

**It places no orders.** There is no `OrderSend` in the file. Run it on a demo
account used purely as a data feed — never on a funded account.

## Why an EA rather than a public API

Free market APIs return delayed data for futures (measured at 10+ minutes) and
none of them quote the same CFD contracts as a broker. The terminal already
holds the exact series being traded, in real time, for nothing. The trade-off is
that it needs a Windows machine running permanently — here, a small VM on the
same host as the API, reachable over the private tailnet.

## Install

1. **File → Open Data Folder** in MT5, then copy this file into `MQL5\Experts\`.
2. Open it in MetaEditor and compile (**F7**). Expect `0 errors, 0 warnings`.
3. **Tools → Options → Expert Advisors**:
   - ☑ Allow algorithmic trading
   - ☑ Allow WebRequest for listed URL
   - add the API base URL **exactly**: scheme, host, port, no trailing slash.
     A mismatch here is the number-one cause of error 4014/4060.
4. Attach the EA to any **M5** chart. A single instance covers every symbol.

## Inputs

| Input | Default | Notes |
|---|---|---|
| `InpApiUrl` | *(placeholder)* | Base URL, no trailing slash. Must match the whitelist entry character for character. |
| `InpIngestToken` | *(empty)* | Same value as `INGEST_TOKEN` in the server's `api.env`. |
| `InpSymbols` | `GER40,US30,NAS100,XAUUSD` | Exact names from Market Watch. Broker suffixes matter (`GER40.cash` ≠ `GER40`). |
| `InpBackfillDays` | `60` | History fetched on first run. Ignored for symbols the server already has, unless `InpForceBackfill` is set. |
| `InpForceBackfill` | `false` | Skip the resume handshake and re-send the whole window. Needed after *widening* `InpBackfillDays` — otherwise the EA resumes from what the server already holds and the longer window is silently ignored. Set back to `false` afterwards. |
| `InpChunkSize` | `500` | Candles per request. The server rejects batches above 2000. |
| `InpMaxChunksPerCycle` | `20` | Requests per symbol per cycle. Spreads a long backfill over several minutes instead of blocking the terminal. `0` disables the cap. |
| `InpTimerSeconds` | `60` | How often to look for newly closed bars. |

For a long backfill, also check **Tools → Options → Charts → Max bars in chart**
is set high (or unlimited): MT5 will not hand `CopyRates` more bars than it
keeps. The first cycles may log `CopyRates n'a rien renvoyé` while the terminal
downloads history from the broker — that is expected, and it resolves itself.

## Time handling — the part that actually matters

MT5 stamps bars in **broker server time**, typically UTC+2 or UTC+3, shifting
with the broker's own DST calendar. The backend stores UTC and slices sessions
by real market timezones (`Europe/London`, `America/New_York`, `Asia/Tokyo`).

Sending server time unconverted would file every candle into the wrong session —
an error that looks entirely plausible on a chart while quietly corrupting months
of levels. So the EA computes the offset from `TimeGMT() - TimeTradeServer()`,
rounded to the nearest 15 minutes to absorb quote jitter, and converts before
sending. As a second line of defence the server rejects any timestamp more than
30 minutes in the future, which is exactly what a missing conversion looks like.

The currently forming bar is never sent: only bars strictly older than
`iTime(symbol, PERIOD_M5, 0)` are transmitted, so a half-built candle can never
be stored and then left uncorrected.

## Restarts and replays

Ingestion is idempotent — `(symbol, ts)` is the primary key and writes are
UPSERTs. On startup the EA calls `/ingest/status` and resumes from the last
candle the server holds, so a terminal restart costs nothing. If that call
fails it simply refetches the full window; the result is identical, just slower.

## Reading the log

Open the **Experts** tab in the terminal.

| Message | Meaning |
|---|---|
| `démarré · 4 symbole(s) · décalage serveur -> UTC = -7200 s` | Broker is UTC+2. Normal. |
| `reprise après 2026-08-30 07:00:00 UTC` | Resuming from server state, no full backfill. |
| `ERREUR 4014` / `ERREUR 4060` | URL not whitelisted, or not an exact match. |
| `envoi refusé (HTTP 401)` | `InpIngestToken` does not match `INGEST_TOKEN`. |
| `envoi refusé (HTTP 400) … is in the future` | Time conversion is wrong — report the log. |
| `CopyRates n'a rien renvoyé` | History still downloading, or the symbol name is wrong. |
