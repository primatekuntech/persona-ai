# Sprint 7 — Polish & Deploy

**Goal:** production-ready on a single VPS. Hardened, observable, backed up, served behind HTTPS. First real invitee can use it.

**Duration estimate:** 4–5 days.

## Deliverables

1. Production Dockerfile (multi-stage, slim runtime).
2. `docker-compose.prod.yml` with Caddy + auto HTTPS.
3. systemd option for users who prefer no docker.
4. Backups: `pg_dump` nightly + `rsync` of `/data/uploads`.
5. Observability: health checks, structured logs, minimal metrics.
6. Security hardening: headers, strict CORS, CSRF on state-changing endpoints, upload size limits at proxy.
7. Self-service data rights: full-account export (`.zip`) and account deletion.
8. Documentation: deployment runbook.

## Hardening

### 7.1 HTTP security headers

Via `tower-http::SetResponseHeader` middleware:

```
Content-Security-Policy: default-src 'self'; img-src 'self' data:; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'
Strict-Transport-Security: max-age=31536000; includeSubDomains
X-Content-Type-Options: nosniff
X-Frame-Options: DENY
Referrer-Policy: same-origin
Permissions-Policy: camera=(), microphone=(), geolocation=()
```

Frontend is served from the same origin as the API in prod, so CSP stays tight.

### 7.2 CORS

Production: no CORS (same-origin). Dev: allow `http://localhost:5173` with `credentials: true`. Config-driven.

### 7.3 CSRF

`SameSite=Lax` already blocks the simple cross-site POST form attack. For defence-in-depth (sub-domain takeover, Safari's historically permissive `Lax` interpretation on top-level navigation), add a **double-submit cookie**:

1. On any authenticated GET that returns HTML or whenever `/api/auth/me` is hit, set a non-HttpOnly cookie `pai_csrf=<32-byte-hex-random>; Path=/; Secure; SameSite=Lax`. Rotated on login and on `sessions/revoke-all`; otherwise stable for the session's life.
2. All state-changing requests (`POST`, `PATCH`, `DELETE`, `PUT`) — **except** `POST /api/auth/login`, `POST /api/auth/password/forgot`, `POST /api/auth/password/reset`, and `POST /api/invites/accept` (which have no prior authenticated session to read the cookie from) — must echo the cookie value as an `X-CSRF-Token` header.
3. Middleware compares `cookie == header` with constant-time comparison. Mismatch → 403 `{ "error": { "code": "csrf_failed" } }`.

Because the cookie is **not HttpOnly**, our own SPA can read it via `document.cookie` and attach it to requests. A cross-site attacker cannot read cookies on our domain, so they cannot replay the header value. Same-origin JS can, but same-origin JS isn't a CSRF threat in the first place.

Implementation note: write a small axum layer `CsrfLayer` that skips enforcement for the listed unauth endpoints. The frontend's `api()` helper (sprint 1) reads `pai_csrf` once and attaches it on every mutating request.

### 7.4 Upload limits

- Reverse proxy caps request body size: Caddy `request_body { max_size 600MB }`.
- Axum `DefaultBodyLimit::max(600 * 1024 * 1024)` on upload routes; default body limit 1 MB elsewhere.
- Per-endpoint rate limits (see sprint 1 + add `POST /api/personas/:id/documents` at 60 req/hour/user).

### 7.5 Secrets

- Secrets never in the repo, never in `app.toml`.
- `.env` with 0600 permissions on the VPS.
- Session secret: 64 hex chars (32 bytes) generated with `openssl rand -hex 32`.
- Rotate `RESEND_API_KEY` separately in Resend; rotating `SESSION_SECRET` invalidates all sessions by design.

### 7.6 Input validation

Every request body validated before touching repositories. `validator` crate or hand-rolled. Reject unknown fields with `serde(deny_unknown_fields)`.

### 7.7 Admin account protection

- Require strong password (≥ 12 chars, complexity optional; NIST-style length over symbol rules).
- Audit log every admin action.
- First admin created only once via bootstrap env; subsequent admins promoted from within the admin UI.
- **No impersonation.** Admins never log in as another user. Debugging uses the read-only admin views below, never acting on the user's behalf. See [`../08-security.md`](../08-security.md#admin-impersonation-forbidden) for the rationale.

### 7.7.1 Admin views (read-only)

Extend the admin UI shipped in sprint 1:

- `/admin/users` — already listed. Add: per-user quota usage (`current_storage_bytes / quota_storage_bytes`), current_doc_count, last_login_at. Actions: disable/enable, role toggle, raise quota (patches `users.quota_storage_bytes`), one-time password-reset link.
- `/admin/jobs` — live queue dashboard. Counts by `(kind, status)`, oldest queued age, longest-running job. Powered by a 10 s polling query. "Force retry" button on `failed` rows; "Cancel" on `queued` rows.
- `/admin/errors` — paginated view of the `errors` table (see [`../01-architecture.md`](../01-architecture.md#error-tracking)): route, code, message, count (rolled up by `(route, code)` for the last 24 h), expand to see full backtrace + request_id. Older than 30 days auto-pruned by the cleanup job (§7.21).
- `/admin/audit` — filtered view of `audit_log`. Filters: actor user, action type, date range. Export CSV for a year's worth.

All views strictly read-only — no mutations possible except the explicit user/job management actions above.

## Deployment

### 7.8 Dockerfile (backend)

```dockerfile
# build
FROM rust:1-bookworm AS build
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential cmake pkg-config libssl-dev libclang-dev \
    ffmpeg ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY backend ./backend
COPY backend/migrations ./backend/migrations
WORKDIR /app/backend
RUN cargo build --release

# runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ffmpeg libssl3 ca-certificates tini \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/backend/target/release/persona-ai /usr/local/bin/persona-ai
COPY --from=build /app/backend/migrations /app/migrations
ENV MIGRATIONS_DIR=/app/migrations
EXPOSE 8080
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/usr/local/bin/persona-ai"]
```

Final image ~ 250 MB without models (models mounted from host volume).

### 7.9 Dockerfile (frontend)

Static build served by Caddy. Two-stage:

```dockerfile
FROM node:20-slim AS build
WORKDIR /app
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN corepack enable && pnpm install --frozen-lockfile
COPY frontend ./
RUN pnpm build

FROM caddy:2-alpine
COPY --from=build /app/dist /srv
COPY docker/Caddyfile /etc/caddy/Caddyfile
```

### 7.10 Caddyfile

```
{
  email you@example.com
}

app.example.com {
  encode zstd gzip

  root * /srv
  file_server

  handle /api/* {
    reverse_proxy backend:8080
  }

  header /api/* {
    -Server
  }

  request_body {
    max_size 600MB
  }

  tls {
    protocols tls1.2 tls1.3
  }
}
```

### 7.11 docker-compose.prod.yml

```yaml
services:
  db:
    image: pgvector/pgvector:pg16
    restart: always
    environment:
      POSTGRES_USER: persona
      POSTGRES_PASSWORD_FILE: /run/secrets/db_password
      POSTGRES_DB: persona
    volumes:
      - pgdata:/var/lib/postgresql/data
    secrets: [db_password]

  backend:
    image: persona-ai/backend:latest
    restart: always
    env_file: /opt/persona-ai/.env
    depends_on: [db]
    volumes:
      - /data:/data

  web:
    image: persona-ai/frontend:latest
    restart: always
    depends_on: [backend]
    ports: ["80:80", "443:443"]
    volumes:
      - caddy_data:/data
      - caddy_config:/config

volumes:
  pgdata:
  caddy_data:
  caddy_config:

secrets:
  db_password:
    file: /opt/persona-ai/secrets/db_password
```

### 7.12 systemd alternative (optional)

For users who prefer no docker, a `persona-ai.service` unit runs the binary directly. Postgres installed via distro package. Caddy as a separate service. Provide both paths in the runbook; docker is the primary.

### 7.13 VPS sizing

- **Minimum:** 4 vCPU, 16 GB RAM, 80 GB SSD.
- **Comfortable:** 8 vCPU, 32 GB RAM, 160 GB SSD.
- Providers that hit this well on cost: Hetzner (`CCX13` or `CCX23`), OVH (`Rise`/`VPS Pro`), Contabo. Avoid anything with < 4 GB RAM — the LLM alone wants 5–6 GB quantised.

## Backups

### 7.14 Database backups

Nightly `pg_dump` to `/data/backups/db/`:

```bash
pg_dump -Fc -Z 9 -f /data/backups/db/persona-$(date +%F).dump $DATABASE_URL
find /data/backups/db -name '*.dump' -mtime +30 -delete
```

Schedule via host cron (not inside a container). Test a restore quarterly.

### 7.15 File backups

`rsync` `/data/uploads`, `/data/transcripts`, `/data/avatars` to off-site storage (another VPS, Backblaze B2, or S3) nightly. Model files are excluded (re-downloadable).

### 7.16 Offsite rotation

Retain 30 daily + 12 monthly. Encrypt with `age` before upload if destination is untrusted.

## Observability

### 7.17 Health checks

- `GET /healthz` — 200 if DB reachable, migrations applied, required model files present. Caddy uses it for upstream health.
- `GET /readyz` — 200 once the LLM is loaded and at least one worker is idle.

### 7.18 Logs

- `tracing-subscriber` JSON in prod.
- Docker: `json-file` driver with 10 MB × 5 rotation, or ship to journald.
- Log fields: `request_id`, `user_id` (if authenticated), `span`, `event`, `elapsed_ms`.
- Do not log request bodies or message content.

### 7.19 Metrics (optional v1)

`axum-prometheus` or hand-rolled counters:

- `http_requests_total{route,status}`
- `ingest_jobs_total{kind,status}`
- `generation_tokens_total`
- `generation_latency_seconds_bucket`

Scrape `/metrics` behind admin-only auth. Pair with a single-node Prometheus + Grafana if the user wants, otherwise skip.

### 7.20 Alerting

Minimal: a cron that curls `/healthz` every 5 minutes and sends email via Resend on failure. Separate check for `/readyz` (LLM + workers). If disk usage on `/data` crosses 85 %, email the admin (see [`../07-data-lifecycle.md`](../07-data-lifecycle.md#global-vps-watchdog)).

### 7.21 Filesystem & table cleanup job

A daily `cron`-scheduled job (tokio `JoinSet` scheduled at 03:00 local, no external cron container):

1. **Orphan files.** Walk `/data/uploads/<persona>/` and `/data/transcripts/` and `/data/avatars/`. For each file, look up the matching `documents` or `personas` row. If missing → delete the file. Catches races where async cleanup in sprint 2/3 dropped a file on the floor.
2. **Orphan DB rows.** For every `documents` row whose `status = 'done'` or `'failed'` but file path is missing → log a warning; do not auto-delete the row.
3. **Prune `errors` older than 30 days.**
4. **Prune `audit_log` older than 180 days.**
5. **Prune `jobs` with `status in ('done','failed')` and `updated_at < now() - 30 days`.**
6. **Prune `idempotency_keys` older than 24 h** (also runs hourly for tighter bounds).
7. **Prune `login_attempts` older than 24 h.**
8. **Prune `password_resets` older than 7 days.**
9. **Prune `invite_tokens` older than `expires_at + 7 days`** — retains a week of audit visibility on used tokens.
10. **Prune orphan temp uploads under `/data/uploads/.tmp/`** older than 1 h (abandoned multipart uploads).

Each step logs summary counts to `tracing`. Failures in one step do not abort the others. The full pass takes < 30 s on a 10k-user dataset.

## Self-service data rights

See [`../07-data-lifecycle.md`](../07-data-lifecycle.md) for the policy commitments this implements.

### 7.22 Full-account export

```
POST   /api/auth/export                          → 202 { job_id }
GET    /api/auth/export/:job_id                  → { status, download_url? }
GET    /api/auth/export/:job_id/download         → application/zip
```

- `POST` enqueues a `user_export` job on the background worker. Two caps enforced server-side:
  - At most 1 pending or running export per user → returns 409 `conflict` if a prior export is still in flight.
  - At most 1 export **per 24 hours** per user → returns 429 `rate_limited` with `Retry-After` set to the remaining window (matches [`../07-data-lifecycle.md`](../07-data-lifecycle.md#data-export-portability)).
- Worker writes a deterministic `.zip` to `/data/exports/<user_id>/<job_id>.zip` containing:
  - `account.json` — user row (sans `password_hash`), quotas, `created_at`, `last_login_at`.
  - `personas/<persona_id>/persona.json` — persona row + associated eras + `style_profile.json` (current whole-persona profile, plus any per-era profiles).
  - `personas/<persona_id>/documents/<doc_id>.{txt,transcript.txt,original.<ext>}` — originals + transcripts, plus a `documents.json` manifest per persona.
  - `personas/<persona_id>/chats/<session_id>.json` — full message history with timestamps + citation `retrieved_chunk_ids`.
  - `audit_log.json` — this user's rows only.
- Job payload is `{ user_id }`. Progress reported via `progress_pct` on the `jobs` row; the GET endpoint translates to `{ status: 'running' | 'done' | 'failed', progress_pct, download_url? }`.
- Download URL is issued as a short-lived signed path (HMAC over `job_id + user_id + expires_at`, 15-min TTL). Serving handler re-checks ownership against the session, so even an exposed URL cannot leak between accounts.
- Export files auto-expire: cleanup job (§7.21) deletes files under `/data/exports/` older than 7 days and marks the job `expired` in its payload metadata.
- Idempotency-Key accepted on `POST` per [`../06-api-conventions.md`](../06-api-conventions.md#idempotency).

Acceptance:
- User with 3 personas + 50 documents + 10 chat sessions → `POST /api/auth/export` returns 202; GET polls; eventually returns a download URL; unzipped archive contains every file and row belonging to the user and **nothing** belonging to others.
- Requesting another user's export `job_id` returns 404 (per [`../06-api-conventions.md`](../06-api-conventions.md#status-codes) 404-not-403 rule).

### 7.23 Account self-delete

```
POST   /api/auth/delete                          { password, confirm: "DELETE" }
                                                 → 202 { job_id }
```

- Requires fresh password re-verification (hash-compared via `argon2::verify`) and the literal string `"DELETE"` in `confirm`; otherwise 400 `validation_field`.
- Admin accounts cannot self-delete while they are the sole remaining admin (409 `last_admin`, matching [`../07-data-lifecycle.md`](../07-data-lifecycle.md#self-delete)). The last-admin check is re-run inside the job transaction to avoid a TOCTOU race.
- Flow:
  1. Handler invalidates all sessions for the user (delete from `session_index` + `tower_sessions` WHERE id IN index).
  2. Flips `users.status = 'disabled'` and sets `users.email = 'deleted-<user_id>@invalid'` so the email can be re-used. Password hash zeroed.
  3. Enqueues a `user_delete` job containing `{ user_id }`.
- Worker handles the job:
  1. Delete personas (cascades documents → chunks, chat_sessions → messages, style_profiles, eras).
  2. Delete on-disk files under `/data/uploads/<persona_id>/`, `/data/transcripts/` (matching doc ids), `/data/avatars/<persona_id>.*`, `/data/exports/<user_id>/`.
  3. Delete `audit_log`, `login_attempts`, `password_resets`, `invite_tokens` (issued **by** user), `idempotency_keys`, `errors` rows for this user.
  4. Delete the `users` row (ON DELETE CASCADE handles FKs we missed).
- Once the job completes the account is irrecoverable. No soft-delete, no undo window (documented in UI copy).

Acceptance:
- Self-delete with wrong password → 400 `validation_field`, account untouched.
- Self-delete as the only admin → 409 `last_admin`, account untouched.
- Successful self-delete → within 60 s all rows + files for that user are gone; a fresh invite to the same email succeeds; the prior user's data is not visible.

## Runbook (`docs/runbook.md`, written in this sprint)

- Fresh install on a new Ubuntu 22.04 VPS (step-by-step).
- Domain + DNS setup.
- First admin creation.
- How to rotate keys.
- How to restore from backup.
- How to upgrade (`docker compose pull && up -d`).
- How to change the default LLM model (swap GGUF file, set `MODEL_PATH`, restart).

## Acceptance tests

1. Fresh Ubuntu 22.04 VPS → follow the runbook → site is reachable over HTTPS in under 30 minutes.
2. `pg_dump` + restore reproduces all data; hashed passwords still verify.
3. `curl -I https://app/...` shows all expected security headers (CSP, HSTS, X-Content-Type-Options, X-Frame-Options, Referrer-Policy, Permissions-Policy).
4. Killing the backend container → Caddy returns 502 briefly; container restarts; site back in < 10 s.
5. Uploading a 500 MB mp3 via the UI works; 600 MB upload is rejected at the proxy with a clear error.
6. Disabled user cannot log in; admin can re-enable.
7. A forged cross-origin POST to `/api/personas` without `X-CSRF-Token` returns 403 `csrf_failed` even with a valid session cookie.
8. Password-reset flow end-to-end: request from `/forgot-password` → Resend email → click link → set new password → all prior sessions invalidated.
9. Admin views show quotas per user, retry a failed job, and list recent 5xx errors grouped by route+code.
10. Manually drop a file from `/data/uploads/` without deleting its DB row → cleanup job logs the orphan; manually delete a documents row without its file → next cleanup pass removes the file.
11. Forge a job in `running` state with `started_at = now() - 20 min` → reaper (sprint 3) resets it to `queued`; next worker picks it up.

## Out of scope

- Horizontal scaling / multi-node.
- Blue-green deploys.
- PITR (point-in-time recovery) beyond daily snapshots.
- Formal pentest.
- External metrics pipeline (Grafana/Loki). `/metrics` exists for opt-in; no first-class dashboard.
