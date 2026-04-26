#!/usr/bin/env bash
# Run chmod +x scripts/backup.sh after cloning.
# Example crontab entry:
#   0 2 * * * DB_CONTAINER=persona-ai-db /opt/persona-ai/scripts/backup.sh >> /var/log/persona-ai-backup.log 2>&1
set -euo pipefail

BACKUP_DIR="${BACKUP_DIR:-/data/backups}"
CONTAINER="${DB_CONTAINER:-persona-ai-db}"
DB_USER="${POSTGRES_USER:-persona}"
DB_NAME="${POSTGRES_DB:-persona}"
DAYS_TO_KEEP="${DAYS_TO_KEEP:-30}"

DATE=$(date +%F)
DB_BACKUP_DIR="$BACKUP_DIR/db"
mkdir -p "$DB_BACKUP_DIR"

echo "[$(date -u +%FT%TZ)] Starting backup..."

# Database dump
podman exec "$CONTAINER" pg_dump -U "$DB_USER" -Fc -Z 9 "$DB_NAME" \
  > "$DB_BACKUP_DIR/persona-$DATE.dump"
echo "[$(date -u +%FT%TZ)] Database dump: $DB_BACKUP_DIR/persona-$DATE.dump"

# Rotate old dumps
find "$DB_BACKUP_DIR" -name '*.dump' -mtime +"$DAYS_TO_KEEP" -delete
echo "[$(date -u +%FT%TZ)] Old dumps pruned (>${DAYS_TO_KEEP} days)"

# File rsync (if RSYNC_DEST is set)
if [ -n "${RSYNC_DEST:-}" ]; then
  rsync -az --exclude='models/' /data/uploads /data/transcripts /data/avatars \
    "$RSYNC_DEST/persona-ai/"
  echo "[$(date -u +%FT%TZ)] Files synced to $RSYNC_DEST"
fi

echo "[$(date -u +%FT%TZ)] Backup complete."
