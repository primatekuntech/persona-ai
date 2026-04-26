# Deployment Runbook

## Prerequisites

- Ubuntu 22.04 LTS or Debian 12
- Podman 4.4+ (`apt install podman` or via Podman's official repo)
- `podman compose` built-in (Podman 4.9+) or `pip3 install podman-compose`
- A domain name with A/AAAA records pointing to your VPS

## 1. VPS sizing

| Tier | vCPU | RAM | SSD |
|------|------|-----|-----|
| Minimum | 4 | 16 GB | 80 GB |
| Recommended | 8 | 32 GB | 160 GB |

Tested on Hetzner CCX13 / CCX23.

## 2. System preparation

Rootless Podman + low-numbered port binding:

```bash
# Allow binding to ports 80/443 without root
sudo sysctl -w net.ipv4.ip_unprivileged_port_start=80
echo 'net.ipv4.ip_unprivileged_port_start=80' | sudo tee /etc/sysctl.d/99-podman-ports.conf

# Create data directory
sudo mkdir -p /data/{uploads,transcripts,avatars,exports,backups,models}
sudo chown -R $USER:$USER /data
```

## 3. Clone and build

```bash
git clone <repo> /opt/persona-ai
cd /opt/persona-ai

# Build backend image (uses repo-root Dockerfile)
podman build -t localhost/persona-ai/backend:latest -f Dockerfile .

# Build frontend image (Caddy-based, includes Caddyfile)
podman build -t localhost/persona-ai/frontend:latest -f docker/Dockerfile.frontend .
```

## 4. Configure

```bash
mkdir -p /opt/persona-ai/secrets

# DB password — written to a file so the container reads it via POSTGRES_PASSWORD_FILE
echo "$(openssl rand -hex 32)" > /opt/persona-ai/secrets/db_password
chmod 600 /opt/persona-ai/secrets/db_password

# Application env — backend reads these at startup
cat > /opt/persona-ai/.env << 'EOF'
DATABASE_URL=postgresql://persona:<DB_PASSWORD>@db:5432/persona
SESSION_SECRET=<64-hex-chars: openssl rand -hex 32>
RESEND_API_KEY=re_xxxx
RESEND_FROM=noreply@yourdomain.com
APP_BASE_URL=https://yourdomain.com
DATA_DIR=/data
MODEL_DIR=/data/models
RUST_ENV=production
ADMIN_BOOTSTRAP_EMAIL=you@yourdomain.com
ADMIN_BOOTSTRAP_PASSWORD=<strong-password>
EOF
chmod 600 /opt/persona-ai/.env

# Caddy env — used by the web container
cat > /opt/persona-ai/caddy.env << 'EOF'
APP_DOMAIN=yourdomain.com
ACME_EMAIL=you@yourdomain.com
EOF

# db.env — used by the Quadlet db unit (Compose reads db_password file directly)
echo "POSTGRES_PASSWORD=$(cat /opt/persona-ai/secrets/db_password)" \
  > /opt/persona-ai/db.env
chmod 600 /opt/persona-ai/db.env
```

Replace `<DB_PASSWORD>` in `DATABASE_URL` with the value in `secrets/db_password`.

## 5. Start with podman compose

```bash
cd /opt/persona-ai
APP_DOMAIN=yourdomain.com ACME_EMAIL=you@yourdomain.com \
  podman compose -f compose.prod.yml up -d
```

Check logs:

```bash
podman compose -f compose.prod.yml logs -f backend
```

## 6. First admin

The admin account is created automatically on first startup via the
`ADMIN_BOOTSTRAP_EMAIL` + `ADMIN_BOOTSTRAP_PASSWORD` env vars. Log in at
`https://yourdomain.com/login`.

## 7. Backups

```bash
# Make executable (done once after cloning)
chmod +x /opt/persona-ai/scripts/backup.sh

# Test a manual run
DB_CONTAINER=persona-ai-db /opt/persona-ai/scripts/backup.sh

# Add to crontab (daily at 02:00)
crontab -e
# Add this line:
# 0 2 * * * DB_CONTAINER=persona-ai-db /opt/persona-ai/scripts/backup.sh >> /var/log/persona-ai-backup.log 2>&1
```

Optional off-site sync — set `RSYNC_DEST=user@host:/backups` before the script
invocation to rsync uploads/transcripts/avatars after the DB dump.

## 8. Health alerting

```bash
# Make executable (done once after cloning)
chmod +x /opt/persona-ai/scripts/healthcheck.sh

# Add to crontab (every 5 minutes)
crontab -e
# Add this line:
# */5 * * * * HEALTH_URL=http://localhost:8080/healthz RESEND_API_KEY=re_xxx ALERT_TO=you@example.com ALERT_FROM=noreply@yourdomain.com /opt/persona-ai/scripts/healthcheck.sh
```

The script sends one alert per outage and suppresses duplicates until the
service recovers.

## 9. Systemd via Quadlet (alternative to podman compose)

Quadlet is the Podman-native approach (Podman 4.4+) that integrates containers
directly into systemd without a compose runtime.

```bash
# Copy unit files into the Quadlet search path (rootless)
mkdir -p ~/.config/containers/systemd
cp /opt/persona-ai/deploy/*.container \
   /opt/persona-ai/deploy/*.network \
   ~/.config/containers/systemd/

# Reload so the generator produces .service units
systemctl --user daemon-reload

# Enable and start (order matters: db first, then backend, then web)
systemctl --user enable --now persona-ai-db
systemctl --user enable --now persona-ai-backend
systemctl --user enable --now persona-ai-web

# Tail logs
journalctl --user -u persona-ai-backend -f
```

For root (system-wide), copy to `/etc/containers/systemd/` and use `systemctl`
without `--user`. See `deploy/README.md` for details.

## 10. Upgrading

```bash
cd /opt/persona-ai
git pull

# Rebuild images
podman build -t localhost/persona-ai/backend:latest -f Dockerfile .
podman build -t localhost/persona-ai/frontend:latest -f docker/Dockerfile.frontend .

# Rolling restart (DB stays up)
podman compose -f compose.prod.yml up -d --no-deps backend web
```

## 11. Restore from backup

```bash
# Stop the app, keep DB running
podman compose -f compose.prod.yml stop backend web

# Drop and recreate the database
podman exec -it persona-ai-db psql -U persona postgres \
  -c "DROP DATABASE persona;" \
  -c "CREATE DATABASE persona;"

# Restore from a custom-format dump
podman exec -i persona-ai-db \
  pg_restore -U persona -d persona \
  < /data/backups/db/persona-2026-01-01.dump

# Restart everything
podman compose -f compose.prod.yml up -d
```

## 12. Rotate SESSION_SECRET

Rotating the secret invalidates all active sessions — every user must log in again.

```bash
new_secret=$(openssl rand -hex 32)
sed -i "s/SESSION_SECRET=.*/SESSION_SECRET=$new_secret/" /opt/persona-ai/.env
podman compose -f compose.prod.yml restart backend
```

## 13. Change LLM model

```bash
# Copy new GGUF into the model directory
cp new-model.gguf /data/models/llm/

# Update MODEL_PATH in .env if the filename changed
sed -i "s|MODEL_PATH=.*|MODEL_PATH=/data/models/llm/new-model.gguf|" /opt/persona-ai/.env

# Restart backend to load the new model
podman compose -f compose.prod.yml restart backend
```

## 14. Firewall

Only ports 80 and 443 need to be publicly reachable. Postgres and the backend
port (8080) are not exposed externally.

```bash
# UFW example
sudo ufw allow 80/tcp
sudo ufw allow 443/tcp
sudo ufw allow 443/udp   # HTTP/3 QUIC
sudo ufw enable
```

## 15. Observability

Logs are structured JSON in production (`RUST_ENV=production`).  Stream them
with:

```bash
podman logs -f persona-ai-backend | jq .
```

No external telemetry is sent — privacy is load-bearing.
