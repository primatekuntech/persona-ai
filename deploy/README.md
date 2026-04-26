# Quadlet — Podman-native systemd units

Quadlet is the Podman-native approach (Podman 4.4+) for running containers as
systemd services, without needing `podman-compose` or a Docker Compose
runtime.  Each `.container` file describes a container unit; `.network`
describes a shared network.  `systemd-generator` converts them into real
`.service` units on `daemon-reload`.

## Files

| File | Purpose |
|------|---------|
| `persona-ai-db.container` | PostgreSQL + pgvector database |
| `persona-ai-backend.container` | Rust/axum backend on 127.0.0.1:8080 |
| `persona-ai-web.container` | Caddy reverse proxy + SPA on 80/443 |
| `persona-ai.network` | Shared bridge network for inter-container DNS |

## Usage

```bash
# Copy unit files into the Quadlet search path (rootless user units)
mkdir -p ~/.config/containers/systemd
cp deploy/*.container deploy/*.network ~/.config/containers/systemd/

# Reload systemd so the generator runs
systemctl --user daemon-reload

# Enable + start
systemctl --user enable --now persona-ai-db persona-ai-backend persona-ai-web

# Check status
systemctl --user status persona-ai-backend
journalctl --user -u persona-ai-backend -f
```

For root (system-wide) deployment, copy to `/etc/containers/systemd/` and use
`systemctl` (without `--user`).

## Environment files expected

| Unit | EnvironmentFile |
|------|----------------|
| `persona-ai-db` | `/opt/persona-ai/db.env` — contains `POSTGRES_PASSWORD=…` |
| `persona-ai-backend` | `/opt/persona-ai/.env` — full app env (see runbook) |
| `persona-ai-web` | `/opt/persona-ai/caddy.env` — `APP_DOMAIN=` and `ACME_EMAIL=` |

See `docs/runbook.md` for full setup instructions.
