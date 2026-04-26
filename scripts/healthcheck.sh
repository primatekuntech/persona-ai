#!/usr/bin/env bash
# Simple health alerting: curl /healthz, email on failure via Resend.
# Run chmod +x scripts/healthcheck.sh after cloning.
# Example crontab entry:
#   */5 * * * * HEALTH_URL=http://localhost:8080/healthz RESEND_API_KEY=re_xxx ALERT_TO=you@example.com ALERT_FROM=noreply@yourdomain.com /opt/persona-ai/scripts/healthcheck.sh
set -euo pipefail

URL="${HEALTH_URL:-http://localhost:8080/healthz}"
RESEND_API_KEY="${RESEND_API_KEY:-}"
ALERT_TO="${ALERT_TO:-}"
ALERT_FROM="${ALERT_FROM:-}"
MARKER="/tmp/persona-ai-health-alert-sent"

if curl -fs --max-time 5 "$URL" > /dev/null 2>&1; then
  rm -f "$MARKER"
  exit 0
fi

# Only alert once (clear on recovery above)
if [ -f "$MARKER" ]; then
  exit 1
fi
touch "$MARKER"

if [ -n "$RESEND_API_KEY" ] && [ -n "$ALERT_TO" ]; then
  curl -s -X POST https://api.resend.com/emails \
    -H "Authorization: Bearer $RESEND_API_KEY" \
    -H "Content-Type: application/json" \
    -d "{\"from\":\"$ALERT_FROM\",\"to\":[\"$ALERT_TO\"],\"subject\":\"[persona-ai] Health check failed\",\"text\":\"Health check at $URL failed at $(date -u +%FT%TZ).\"}"
fi
exit 1
