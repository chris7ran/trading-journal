#!/usr/bin/env bash
#
# Freeze the brief for every fed symbol, before a trading session opens.
#
# Run from cron twice a day (see the crontab lines at the bottom of this file).
# The API keeps the first capture of a (symbol, day, session) and ignores later
# calls, so a retry after a failure is safe and a duplicate run is harmless.
#
#   ./snapshot-brief.sh pre_london
#   ./snapshot-brief.sh pre_ny
#
set -euo pipefail

SESSION="${1:-}"
case "$SESSION" in
  pre_london|pre_ny|manual) ;;
  *)
    echo "usage: $(basename "$0") <pre_london|pre_ny|manual>" >&2
    exit 2
    ;;
esac

# The API listens on loopback only; TLS is terminated by `tailscale serve` for
# remote clients, so a local caller can and should skip it entirely.
API="${API:-http://127.0.0.1:8090}"
ENV_FILE="${ENV_FILE:-$(cd "$(dirname "$0")/.." && pwd)/api.env}"

if [[ ! -r "$ENV_FILE" ]]; then
  echo "$(date -Is) [snapshot] cannot read $ENV_FILE" >&2
  exit 1
fi

# Read the token without sourcing the file: api.env contains an Argon2 hash
# full of '$', and sourcing it would run the shell over characters it should
# never interpret.
TOKEN="$(sed -n 's/^INGEST_TOKEN=//p' "$ENV_FILE" | tr -d '\r"' | head -n1)"
if [[ -z "$TOKEN" ]]; then
  echo "$(date -Is) [snapshot] INGEST_TOKEN is empty in $ENV_FILE" >&2
  exit 1
fi

# The API defaults `date` to today in Paris terms, which is what both cron
# slots want. Passing it explicitly would only introduce a way to disagree.
RESPONSE="$(curl -sS --max-time 60 \
  -X POST "$API/brief/snapshot" \
  -H 'Content-Type: application/json' \
  -H "X-Ingest-Token: $TOKEN" \
  -d "{\"session\":\"$SESSION\"}" \
  -w '\n%{http_code}')" || {
    echo "$(date -Is) [snapshot] $SESSION — curl failed" >&2
    exit 1
  }

CODE="$(tail -n1 <<<"$RESPONSE")"
BODY="$(sed '$d' <<<"$RESPONSE")"

if [[ "$CODE" != "200" ]]; then
  echo "$(date -Is) [snapshot] $SESSION — HTTP $CODE: $BODY" >&2
  exit 1
fi

echo "$(date -Is) [snapshot] $SESSION — $BODY"

# --- Installation -----------------------------------------------------------
#
# chmod +x snapshot-brief.sh, then `crontab -e`:
#
#   # Freeze the brief before each session. Times are the server's local zone;
#   # check with `timedatectl` that it is Europe/Paris, otherwise adjust.
#   45 7  * * 1-5 /home/chris/stacks/journal/code/backend/deploy/snapshot-brief.sh pre_london >> /home/chris/logs/brief-snapshot.log 2>&1
#   0  15 * * 1-5 /home/chris/stacks/journal/code/backend/deploy/snapshot-brief.sh pre_ny      >> /home/chris/logs/brief-snapshot.log 2>&1
#
# Weekdays only: a weekend capture would archive an empty day, and the API
# skips those anyway.
#
# The hours are deliberately *before* each open — 07:45 leaves the Asian range
# complete and London untouched, 15:00 catches the European morning before New
# York starts. A snapshot taken after the open would already contain what the
# session did, which is precisely what it must not know.
