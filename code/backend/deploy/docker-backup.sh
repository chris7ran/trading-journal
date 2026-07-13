#!/usr/bin/env bash
# Consistent SQLite backup for the DOCKER deployment, with rotation.
# Safe to run while the API is live: sqlite3 ".backup" takes a proper snapshot
# even in WAL mode. Runs a throwaway Alpine container that shares the API
# container's data volume (--volumes-from), so no need to know the volume name.
#
# Env overrides:
#   TJ_CONTAINER    running API container name (default: tjapi)
#   TJ_BACKUP_DIR   where to write backups     (default: $HOME/backups/journal)
#   TJ_BACKUP_KEEP  how many archives to keep  (default: 14)
set -euo pipefail

CONTAINER="${TJ_CONTAINER:-tjapi}"
DEST="${TJ_BACKUP_DIR:-$HOME/backups/journal}"
KEEP="${TJ_BACKUP_KEEP:-14}"

mkdir -p "$DEST"
ts="$(date +%Y%m%d-%H%M%S)"

# Snapshot + gzip inside the container (writes to the host-mounted $DEST).
docker run --rm \
  --volumes-from "$CONTAINER" \
  -v "$DEST":/backup \
  alpine:latest \
  sh -c "apk add --no-cache sqlite >/dev/null 2>&1 && \
         sqlite3 /app/data/journal.db \".backup '/backup/journal-$ts.db'\" && \
         gzip -f '/backup/journal-$ts.db'"

# Rotate: keep the newest $KEEP archives, delete the rest.
ls -1t "$DEST"/journal-*.db.gz 2>/dev/null | tail -n +"$((KEEP + 1))" | xargs -r rm -f

echo "backup ok: $DEST/journal-$ts.db.gz  (garde $KEEP)"
