#!/usr/bin/env bash
#
# seed.sh — populate the Trading Journal backend with demo trades.
#
# What it does:
#   1. Logs in (POST /auth/login) to get a JWT.
#   2. Sends ~10 realistic demo trades (POST /trades).
#
# Idempotent: each trade carries a fixed mt5_ticket, so re-running the script
# skips trades that already exist (the API returns 409 on duplicate tickets).
#
# Usage:
#   BASE_URL=http://localhost:8080 PASSWORD='your-password' ./seed.sh
#
# Defaults: BASE_URL=http://localhost:8080. PASSWORD is required.
#
# Requires: bash, curl, and either jq or python3 (to read the JWT from JSON).

set -euo pipefail

BASE_URL="${BASE_URL:-http://localhost:8080}"
PASSWORD="${PASSWORD:-}"

if [[ -z "$PASSWORD" ]]; then
  echo "ERROR: set PASSWORD, e.g. PASSWORD='my-password' ./seed.sh" >&2
  exit 1
fi

# --- Helper: extract a JSON string field without hard-depending on jq --------
json_field() {
  # usage: json_field <field>   (reads JSON from stdin)
  local field="$1"
  if command -v jq >/dev/null 2>&1; then
    jq -r ".${field} // empty"
  elif command -v python3 >/dev/null 2>&1; then
    python3 -c "import sys,json; print(json.load(sys.stdin).get('${field}',''))"
  else
    echo "ERROR: need jq or python3 to parse JSON" >&2
    exit 1
  fi
}

echo "→ Logging in to ${BASE_URL} ..."
TOKEN="$(
  curl -fsS -X POST "${BASE_URL}/auth/login" \
    -H 'Content-Type: application/json' \
    -d "{\"password\":$(printf '%s' "$PASSWORD" | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))' 2>/dev/null || echo "\"$PASSWORD\"")}" \
  | json_field token
)"

if [[ -z "$TOKEN" ]]; then
  echo "ERROR: login failed (no token). Wrong password or backend unreachable?" >&2
  exit 1
fi
echo "✓ Got token."

# --- Demo trades -------------------------------------------------------------
# Each line is a full JSON body. mt5_ticket keeps the seed idempotent.
TRADES=(
'{"symbol":"GER40","direction":"LONG","open_time":"2026-06-10T09:15:00","close_time":"2026-06-10T10:05:00","open_price":18250,"close_price":18302,"lot_size":1.0,"pnl":520,"commission":-3,"swap":0,"setup_tag":"Break & Retest","emotion_tag":"Confident","mt5_ticket":"SEED-001"}'
'{"symbol":"EURUSD","direction":"SHORT","open_time":"2026-06-10T13:40:00","close_time":"2026-06-10T14:10:00","open_price":1.0905,"close_price":1.0918,"lot_size":0.5,"pnl":-130,"commission":-2,"swap":0,"setup_tag":"Range Fade","emotion_tag":"FOMO","mt5_ticket":"SEED-002"}'
'{"symbol":"XAUUSD","direction":"LONG","open_time":"2026-06-11T08:30:00","close_time":"2026-06-11T11:00:00","open_price":2348.5,"close_price":2355.2,"lot_size":0.3,"pnl":340,"commission":-2,"swap":-1,"setup_tag":"Trend Pullback","emotion_tag":"Calm","mt5_ticket":"SEED-003"}'
'{"symbol":"NAS100","direction":"LONG","open_time":"2026-06-11T15:35:00","close_time":"2026-06-11T16:20:00","open_price":19850,"close_price":19805,"lot_size":0.2,"pnl":-210,"commission":-2,"swap":0,"setup_tag":"Breakout","emotion_tag":"Impatient","mt5_ticket":"SEED-004"}'
'{"symbol":"US30","direction":"SHORT","open_time":"2026-06-12T14:00:00","close_time":"2026-06-12T15:30:00","open_price":40120,"close_price":40060,"lot_size":0.5,"pnl":180,"commission":-3,"swap":0,"setup_tag":"News Fade","emotion_tag":"Confident","mt5_ticket":"SEED-005"}'
'{"symbol":"GER40","direction":"SHORT","open_time":"2026-06-12T09:05:00","close_time":"2026-06-12T09:40:00","open_price":18410,"close_price":18401,"lot_size":1.0,"pnl":95,"commission":-3,"swap":0,"setup_tag":"Open Drive","emotion_tag":"Calm","mt5_ticket":"SEED-006"}'
'{"symbol":"EURUSD","direction":"LONG","open_time":"2026-06-15T10:20:00","close_time":"2026-06-15T12:00:00","open_price":1.0860,"close_price":1.0886,"lot_size":0.5,"pnl":260,"commission":-2,"swap":0,"setup_tag":"Support Bounce","emotion_tag":"Confident","mt5_ticket":"SEED-007"}'
'{"symbol":"XAUUSD","direction":"SHORT","open_time":"2026-06-15T16:10:00","close_time":"2026-06-15T16:45:00","open_price":2360.0,"close_price":2362.5,"lot_size":0.3,"pnl":-75,"commission":-2,"swap":0,"setup_tag":"Range Fade","emotion_tag":"Revenge","mt5_ticket":"SEED-008"}'
'{"symbol":"BTCUSD","direction":"LONG","open_time":"2026-06-16T11:00:00","close_time":"2026-06-16T18:30:00","open_price":68500,"close_price":69220,"lot_size":0.1,"pnl":610,"commission":-5,"swap":-3,"setup_tag":"Trend Pullback","emotion_tag":"Patient","mt5_ticket":"SEED-009"}'
'{"symbol":"GER40","direction":"LONG","open_time":"2026-06-17T09:30:00","close_time":"2026-06-17T10:15:00","open_price":18500,"close_price":18485,"lot_size":1.0,"pnl":-150,"commission":-3,"swap":0,"setup_tag":"Breakout","emotion_tag":"FOMO","mt5_ticket":"SEED-010"}'
)

created=0
skipped=0
failed=0

echo "→ Seeding ${#TRADES[@]} trades ..."
for body in "${TRADES[@]}"; do
  code="$(
    curl -s -o /dev/null -w '%{http_code}' -X POST "${BASE_URL}/trades" \
      -H "Authorization: Bearer ${TOKEN}" \
      -H 'Content-Type: application/json' \
      -d "$body"
  )"
  case "$code" in
    201) created=$((created+1)); echo "  ✓ created ($code)";;
    409) skipped=$((skipped+1)); echo "  • already exists ($code)";;
    *)   failed=$((failed+1));  echo "  ✗ failed ($code): $body";;
  esac
done

echo ""
echo "Done — created: ${created}, skipped: ${skipped}, failed: ${failed}"
echo "Check: curl -s ${BASE_URL}/trades/stats -H 'Authorization: Bearer <token>'"
