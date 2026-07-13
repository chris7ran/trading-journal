#!/usr/bin/env bash
# Consistent SQLite backup with rotation. Safe to run while the API is live
# (sqlite3 ".backup" takes a proper snapshot even with WAL).
#
# Env overrides:
#   TJ_DB          path to the live DB   (default: /var/lib/trading-journal-api/journal.db)
#   TJ_BACKUP_DIR  where to write        (default: /var/backups/trading-journal-api)
#   TJ_BACKUP_KEEP how many to keep      (default: 14)
set -euo pipefail

DB="${TJ_DB:-/var/lib/trading-journal-api/journal.db}"
DEST="${TJ_BACKUP_DIR:-/var/backups/trading-journal-api}"
KEEP="${TJ_BACKUP_KEEP:-14}"

if [ ! -f "$DB" ]; then
  echo "backup: DB introuvable: $DB" >&2
  exit 1
fi

mkdir -p "$DEST"
ts="$(date +%Y%m%d-%H%M%S)"
out="$DEST/journal-$ts.db"

if command -v sqlite3 >/dev/null 2>&1; then
  sqlite3 "$DB" ".backup '$out'"
else
  # Fallback (less safe with WAL, but works): plain copy.
  cp "$DB" "$out"
fi

gzip -f "$out"

# Rotate: keep the newest $KEEP archives, delete the rest.
ls -1t "$DEST"/journal-*.db.gz 2>/dev/null | tail -n +"$((KEEP + 1))" | xargs -r rm -f

echo "backup ok: $out.gz  (garde $KEEP)"
