# Sprint 1 — Foundation: scaffold, auth, invites

**Goal:** a deployed-ready skeleton where an admin can invite a user by email, the invitee sets a password, logs in, sees an empty persona list, and logs out. No ML yet.

**Duration estimate:** 5–7 working days.

## Deliverables

1. Monorepo scaffolded with backend (Rust / Cargo workspace) and frontend (Vite / pnpm).
2. `docker-compose.yml` runs Postgres 16 + pgvector + app locally.
3. **Full init migration** (`20260425000000_init.sql`) covering every table defined in [`../02-data-model.md`](../02-data-model.md) — identity, personas, documents, chunks, jobs, style profiles, chats, errors, idempotency, audit, login_attempts. Extensions: `vector`, `pg_trgm`. Sprint 2+ only add data, never schema (unless the feature is genuinely new).
4. Admin bootstrap from env var.
5. Invite creation + Resend email delivery + self-service password reset.
6. Invite acceptance, login, logout, session management (+ revoke-all).
7. Rate-limiting and per-account brute-force cooldown on auth endpoints.
8. Model-file SHA-256 verification on startup per [`../04-models.md`](../04-models.md#file-integrity) — even before ML is wired up, the loader is present and `/healthz` fails on mismatch.
9. Minimal frontend: `/login`, `/accept-invite`, `/forgot-password`, `/reset-password`, `/personas` (empty state), `/admin/users`, `/admin/invites`.
10. Design tokens and shadcn/ui wired up per [`../03-design-system.md`](../03-design-system.md).

## Repo layout

```
persona-ai/
├── backend/
│   ├── Cargo.toml
│   ├── migrations/
│   │   └── 20260425000000_init.sql
│   └── src/
│       ├── main.rs
│       ├── config.rs
│       ├── error.rs
│       ├── db.rs
│       ├── auth/
│       │   ├── mod.rs
│       │   ├── middleware.rs
│       │   ├── password.rs
│       │   └── session.rs
│       ├── email/
│       │   ├── mod.rs
│       │   └── resend.rs
│       ├── routes/
│       │   ├── mod.rs
│       │   ├── auth.rs
│       │   ├── admin.rs
│       │   └── health.rs
│       ├── repositories/
│       │   ├── mod.rs
│       │   ├── users.rs
│       │   └── invites.rs
│       └── services/
│           ├── mod.rs
│           └── invites.rs
├── frontend/
│   ├── package.json
│   ├── vite.config.ts
│   ├── tailwind.config.js
│   ├── tsconfig.json
│   └── src/
│       ├── main.tsx
│       ├── App.tsx
│       ├── lib/
│       │   ├── api.ts
│       │   ├── auth.ts
│       │   └── theme.ts
│       ├── components/
│       │   └── ui/            # shadcn components
│       ├── pages/
│       │   ├── Login.tsx
│       │   ├── AcceptInvite.tsx
│       │   ├── Personas.tsx
│       │   ├── admin/
│       │   │   ├── Users.tsx
│       │   │   └── Invites.tsx
│       └── styles/
│           └── globals.css
├── docker/
│   ├── Dockerfile.backend
│   └── Dockerfile.frontend
├── docker-compose.yml
├── docker-compose.prod.yml
├── justfile
├── .env.example
├── .gitignore
└── README.md
```

## Backend tasks

### 1.1 Cargo workspace

- `cargo init --bin backend --name persona-ai` then add dependencies per [`../01-architecture.md`](../01-architecture.md) tech stack table. The crate and binary are both named `persona-ai` so the built artifact matches the `/opt/persona-ai/bin/persona-ai` path used in [`../01-architecture.md`](../01-architecture.md#directory-layout-on-disk-vps) and [`sprint-07-polish-deploy.md`](sprint-07-polish-deploy.md).
- `rust-toolchain.toml` pinning stable.

### 1.2 Config

`config.rs` loads from `app.toml` then overrides from env. Required fields:

```rust
pub struct AppConfig {
    pub bind_addr: SocketAddr,                 // default 0.0.0.0:8080
    pub database_url: String,                  // from env DATABASE_URL
    pub session_secret: String,                // from env SESSION_SECRET (64 hex chars)
    pub session_ttl_hours: u64,                // default 24 * 14
    pub resend_api_key: String,                // from env RESEND_API_KEY
    pub resend_from: String,                   // e.g. "noreply@personaai.app"
    pub app_base_url: String,                  // for invite links, e.g. "https://personaai.app"
    pub admin_bootstrap_email: Option<String>, // first-run admin creation
    pub admin_bootstrap_password: Option<String>,
    pub data_dir: PathBuf,                     // default /data
    pub model_dir: PathBuf,                    // default /data/models
    pub worker_threads: usize,                 // default num_cpus::get() - 1
}
```

Panic on startup if any required field is missing.

### 1.3 DB bootstrap

- `sqlx::PgPool` with 10 max connections.
- Run migrations in `main.rs` before binding the HTTP server.
- The single init migration (`backend/migrations/20260425000000_init.sql`) creates **every** table defined in [`../02-data-model.md`](../02-data-model.md) along with its indexes and partial-unique constraints. Extensions enabled up front: `vector`, `pg_trgm`. This avoids per-sprint schema churn and keeps migrations append-only from sprint 2 onward.
- Do **not** create `tower_sessions` manually; `tower-sessions-sqlx-store` owns that table and creates/migrates it itself. We maintain a shadow `session_index(user_id, session_id, created_at)` projection as defined in the data model, updated from the `auth.login` / `auth.logout` code path, not from a DB trigger on a library-owned table.

### 1.4 Admin bootstrap

On startup, if `ADMIN_BOOTSTRAP_EMAIL` is set and no admin exists in `users`, create one with the provided password. Log a clear message: `bootstrap admin created: email=<...>`. Do not run on subsequent starts.

### 1.5 Password hashing

`auth/password.rs`:

```rust
pub fn hash(password: &str) -> Result<String, AppError>;
pub fn verify(password: &str, hash: &str) -> Result<bool, AppError>;
```

Use `argon2::Argon2::default()` (Argon2id, memory 19 MiB, t=2, p=1 — OWASP 2024 recommendation). Use `password_hash::SaltString::generate` with `OsRng`.

### 1.6 Sessions

Use `tower-sessions` with `tower-sessions-sqlx-store::PostgresStore`. Session cookie name `pai_session`. Flags: `HttpOnly; Secure; SameSite=Lax; Path=/`. TTL from config (14-day rolling).

On login:
1. Generate a **new** session id (regeneration prevents fixation).
2. Write `user_id` into session data.
3. Insert a row into our shadow `session_index` table: `(user_id, session_id_hash, created_at, user_agent, ip)`. This is what powers "log out everywhere" — `tower_sessions` is library-owned and we never query its rows directly.
4. Role is **not** cached in session data. Authorization always refetches `users.role` (and `users.status`) per request via `require_auth`, so admin demotion or account disable takes effect immediately without forcing re-login.

On logout:
1. Call `session.delete()` (tower-sessions removes the row from its own table).
2. Delete the matching `session_index` row by `session_id_hash`.

### 1.6.1 Revoke-all

```
POST /api/auth/sessions/revoke-all    204
```

Authenticated endpoint; deletes all of the caller's rows from `session_index` AND from the library-owned `tower_sessions` table (by joining on `session_id_hash`). Used on password change, manual "sign out everywhere", or admin-triggered reset. Audit-logged as `user.sessions_revoked`.

### 1.7 Auth middleware

`require_auth` extractor:

```rust
pub struct UserCtx { pub user_id: Uuid, pub role: Role, pub status: UserStatus }
```

Loads session → **refetches** `users(role, status)` row by `user_id` on every request → injects `UserCtx`. Returns 401 if session missing or user row gone; returns 403 with `{ "error": { "code": "account_disabled" } }` if `status != 'active'`.

The per-request user fetch is the reason we can keep role out of the session: there is no stale-role window. On a moderately sized user table (< 10k) with a primary-key lookup and a connection pool, this is sub-millisecond.

`require_admin` extractor: uses `require_auth`, returns 403 if `role != admin`. The 403 body follows [`../06-api-conventions.md`](../06-api-conventions.md#error-envelope).

### 1.8 Endpoints

```
POST /api/auth/login                   { email, password } → 204 + session cookie
POST /api/auth/logout                  204
POST /api/auth/sessions/revoke-all     204  (logs out all sessions for the caller)
GET  /api/auth/me                      { user_id, email, role, display_name }

POST /api/auth/password/forgot         { email } → 204  (always 204, never reveal existence)
POST /api/auth/password/reset          { token, new_password } → 204 + cookie
                                       (invalidates all existing sessions for that user)

GET  /api/invites/validate?token=      { email, role, expires_at } | 404
POST /api/invites/accept               { token, password, display_name } → 204 + cookie

POST /api/admin/invites                admin; { email, role } → { invite_url }
GET  /api/admin/invites                admin; paginated list
DELETE /api/admin/invites/:id          admin; revoke

GET  /api/admin/users                  admin; paginated list
PATCH /api/admin/users/:id             admin; { status, role }
POST /api/admin/users/:id/reset        admin; returns one-time reset URL

GET  /healthz                          200 if DB + migrations + model hashes ok
GET  /readyz                           200 once LLM loaded + at least one worker idle
```

All `/api/*` responses are JSON with `Content-Type: application/json`.

### 1.9 Invite tokens

- Generate 32 random bytes, hex-encode → 64-char plaintext token.
- Store `sha256(token)` as primary key in `invite_tokens`.
- Default TTL 7 days.
- Invite link: `{APP_BASE_URL}/accept-invite?token={plaintext}`.
- The plaintext is returned once at creation time and never stored.

#### 1.9.1 Edge cases (must be tested)

| Case | Behaviour |
|------|-----------|
| Admin invites an email that matches an existing active user | 409 `{ "error": { "code": "user_exists" } }` — do not send the email. |
| Admin invites an email that matches a disabled user | 409 `{ "error": { "code": "user_exists" } }`. Admin should re-enable, not re-invite. |
| Admin invites an email that has an unused unexpired invite | 409 `{ "error": { "code": "invite_pending" } }`. Enforced by the `invite_tokens_active_email_uniq` partial index in [`../02-data-model.md`](../02-data-model.md). Admin must revoke before re-inviting. |
| Two browsers POST `/api/invites/accept` with the same token simultaneously | Wrap the acceptance in a transaction that does `SELECT ... FROM invite_tokens WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now() FOR UPDATE`. First commit wins, second returns 410 `{ "error": { "code": "invalid_token" } }`. |
| Token expired | 410 `invalid_token`. |
| Token used | 410 `invalid_token` (indistinguishable from expired on purpose). |

### 1.9.2 Password-reset tokens

Same shape as invite tokens: 32 random bytes, hex-encoded, stored as `sha256`. TTL **30 minutes** (matches [`../08-security.md`](../08-security.md#password-reset) — password resets should not linger). Delivered via `ResendClient::send_password_reset`. Reset link: `{APP_BASE_URL}/reset-password?token={plaintext}`.

`/api/auth/password/forgot` **always** returns 204, whether the email exists or not. On a match it inserts a `password_resets` row and sends the email; on a miss it still spends a fake argon2 hash to keep timing constant. Prevents account enumeration.

On successful reset: invalidate all sessions for that user (same flow as `/sessions/revoke-all`), audit-log `password.reset_completed`, set a fresh cookie.

### 1.10 Resend integration

`email/resend.rs`:

```rust
pub struct ResendClient { api_key: String, from: String, http: reqwest::Client }

impl ResendClient {
    pub async fn send_invite(&self, to: &str, invite_url: &str, inviter_name: &str) -> Result<(), AppError>;
    pub async fn send_password_reset(&self, to: &str, reset_url: &str) -> Result<(), AppError>;
}
```

Uses Resend REST API `POST https://api.resend.com/emails` with Bearer token. Templates inline in Rust using `format!` or `minijinja` (keep it simple — two short emails). Subject: `You've been invited to Persona AI`. Body: short plain-text + HTML version with a single CTA button and a plain-text fallback link.

Configure `RESEND_FROM` to a verified domain. In dev, Resend's test API key and `onboarding@resend.dev` from-address are fine.

### 1.11 Rate limiting & brute-force protection

Three distinct mechanisms layered on auth:

**A. Per-IP rate limit (tower-governor).** Mounted on `/api/auth/login`, `/api/auth/password/forgot`, `/api/auth/password/reset`, `/api/invites/accept`, and `/api/admin/invites`. Config: 10 req / 60 s per IP. Response: 429 with `Retry-After`. Catches dumb scraping.

**B. Per-account cooldown (`login_attempts` table).** On every login attempt (successful or not) insert a row `(email_lower, ip, succeeded, attempted_at)`. Before processing a login, count failed rows for that email in the last 15 minutes:
- `< 5` failures → proceed.
- `≥ 5` failures → return 429 with `Retry-After: <seconds until the oldest failure ages out>` and a body of `{ "error": { "code": "rate_limited", "message": "Too many attempts. Try again in N minutes." } }`. Successful login clears the counter (delete that email's failed rows).

Prevents a single attacker distributed across IPs from credential-stuffing one account. Table grows slowly; a nightly cron deletes rows older than 24 h (the per-account count only reads 15-minute windows).

**C. Constant-time comparison & baseline argon2 cost.** Even on "user not found", run a dummy `argon2::verify` against a fixed hash so the response time does not leak existence. Rate limiting above only applies after this, so timing-side-channel enumeration is blocked regardless.

Rate-limiting scope and parameters for non-auth endpoints are listed in [`../06-api-conventions.md`](../06-api-conventions.md#rate-limiting).

### 1.12 Errors

`AppError` enum with variants: `NotFound`, `Unauthorized`, `Forbidden`, `Validation(String)`, `RateLimited`, `Internal(String)`. `IntoResponse` maps to status + JSON `{ "error": "<code>", "message": "<safe msg>" }`. Internal errors log the full chain via `tracing::error!` but the response message is always generic.

### 1.13 Logging

`tracing-subscriber` with env-filter. Dev: pretty. Prod: JSON. `RUST_LOG=info,persona_ai=debug,sqlx=warn` default.

### 1.14 Audit log

Write an `audit_log` row for: `user.login`, `user.login_failed`, `user.logout`, `user.sessions_revoked`, `invite.created`, `invite.accepted`, `invite.revoked`, `user.disabled`, `user.enabled`, `user.role_changed`, `admin.bootstrapped`, `password.reset_requested`, `password.reset_completed`.

### 1.15 Model-file verification on startup

Even though sprint 1 does not load the whisper or LLM models, the integrity loader is written now so that `/healthz` and `/readyz` behave uniformly from day one.

- Read `backend/assets/models.toml` (embedded via `include_str!`) to get the expected `(path, sha256, size_bytes)` for each model.
- For each entry: if `MODEL_DIR/<path>` does not exist, or `size_bytes` differs, or streamed SHA-256 mismatches, log `error!(model = %name, "model integrity check failed")` and mark readiness as **degraded**.
- `/healthz` still returns 200 in degraded mode (DB is up, app is alive) but the body reports `{ "status": "degraded", "missing_models": [...] }`. `/readyz` returns 503 until all models verify.
- In sprint 1, degraded is the normal state because nobody has downloaded the GGUF yet. Running `scripts/download-models.sh` (shipped this sprint) fixes it.

The verifier runs **once at startup**, not per request. Hashing a 4.5 GB GGUF streams in ~15 s on a VPS. Acceptable boot cost; we do not want to re-verify on every health check.

## Frontend tasks

### 1.16 Vite scaffold

- `pnpm create vite@latest frontend -- --template react-ts`
- Add tailwind, postcss, autoprefixer. Configure per the design system.
- Install shadcn CLI, init in `src/components/ui`, add: `button`, `input`, `label`, `card`, `dialog`, `toast`, `dropdown-menu`, `table`.
- Install `react-router-dom`, `@tanstack/react-query`, `zustand`, `react-hook-form`, `zod`, `@hookform/resolvers`, `lucide-react`.

### 1.17 API client

`src/lib/api.ts`:

```ts
const API = import.meta.env.VITE_API_URL;
export async function api<T>(path: string, init: RequestInit = {}): Promise<T> {
  const res = await fetch(`${API}${path}`, { credentials: "include", ...init });
  if (!res.ok) throw await errorFromResponse(res);
  return res.status === 204 ? (undefined as T) : res.json();
}
```

Shared error type, toast on 401/403/5xx.

### 1.18 Auth state

`src/lib/auth.ts`:

```ts
export function useMe() { return useQuery({ queryKey: ["me"], queryFn: () => api<Me>("/api/auth/me") }); }
export function useLogin() { ... }
export function useLogout() { ... }
```

### 1.19 Pages

- **`/login`** — email + password, submits to `/api/auth/login`, redirects to `/personas`. "Forgot password?" link to `/forgot-password`.
- **`/forgot-password`** — email input, submits to `/api/auth/password/forgot`, shows "If an account exists, we've sent a link" regardless of outcome.
- **`/reset-password?token=`** — on mount, shows password + confirm inputs; submits `{token, new_password}` to `/api/auth/password/reset`, then redirects to `/personas`.
- **`/accept-invite?token=`** — on mount, validate token → show email (read-only) + password + display_name → submit → auto-login → redirect `/personas`.
- **`/personas`** — empty state "No personas yet. Create one to get started." with disabled "Create persona" button (implemented in sprint 2).
- **`/settings/account`** — "Log out everywhere" button calling `/api/auth/sessions/revoke-all`; change-password form.
- **`/admin/users`** — table of users, disable/enable, role toggle.
- **`/admin/invites`** — create invite form; list of pending + used.

### 1.20 Layout

Top bar + sidebar per design system. Sidebar shows "Admin" section only if `role === 'admin'`.

## Infra tasks

### 1.21 docker-compose.yml (dev)

```yaml
services:
  db:
    image: pgvector/pgvector:pg16
    environment:
      POSTGRES_USER: persona
      POSTGRES_PASSWORD: persona
      POSTGRES_DB: persona
    volumes:
      - pgdata:/var/lib/postgresql/data
    ports: ["5432:5432"]

  backend:
    build: { context: ., dockerfile: docker/Dockerfile.backend }
    env_file: .env
    depends_on: [db]
    ports: ["8080:8080"]
    volumes: ["./data:/data"]

  frontend:
    build: { context: ., dockerfile: docker/Dockerfile.frontend }
    ports: ["5173:5173"]
    environment:
      VITE_API_URL: http://localhost:8080

volumes: { pgdata: {} }
```

### 1.22 justfile

```
default:
  @just --list

dev:
  docker compose up -d db
  cd backend && cargo watch -x run

web:
  cd frontend && pnpm dev

migrate:
  cd backend && sqlx migrate run

reset-db:
  docker compose down -v && docker compose up -d db
```

### 1.23 .env.example

```
DATABASE_URL=postgres://persona:persona@localhost:5432/persona
SESSION_SECRET=change-me-to-64-hex-chars
RESEND_API_KEY=re_...
RESEND_FROM=onboarding@resend.dev
APP_BASE_URL=http://localhost:5173
ADMIN_BOOTSTRAP_EMAIL=you@example.com
ADMIN_BOOTSTRAP_PASSWORD=change-me
```

## Acceptance tests

1. `docker compose up` from a clean checkout brings the app up. Opening `http://localhost:5173/login` shows the login form.
2. With `ADMIN_BOOTSTRAP_EMAIL/PASSWORD` set, the admin can log in on first run.
3. Admin creates an invite → receives the invite link in the response → email arrives in Resend test inbox.
4. Opening the invite link in a new browser prompts for password + display name. Submit logs the new user in.
5. New user cannot access `/admin/*` (403).
6. Login, logout, login again → session persists on reload; logout clears session.
7. 11 rapid login attempts from one IP → 11th returns 429 (IP limiter).
8. 6 failed logins for one email from rotating IPs within 15 min → 6th returns 429 with a `Retry-After` pointing at the oldest failure's age-out (per-account cooldown).
9. Admin inviting an email that already has an active user → 409 `user_exists`, no email sent.
10. Admin inviting an email that already has an unused unexpired invite → 409 `invite_pending`.
11. Two concurrent `POST /api/invites/accept` with the same token → first returns 204, second returns 410 `invalid_token` (no duplicate user created).
12. `/api/auth/password/forgot` for a **non-existent** email returns 204 in ~the same time as for an existing email (timing parity within 50 ms).
13. Password reset flow end-to-end: request → email → click link → set new password → old sessions in a second browser are invalidated on next request.
14. Admin demotes a logged-in admin user → their next request returns 403 (no re-login needed — role not cached in session).
15. Boot the app without `MODEL_DIR` populated → `/healthz` 200 with `"status":"degraded"`, `/readyz` 503.
16. Boot with a tampered LLM file (flip one byte) → `/readyz` 503 with a log line naming the offending model.
17. Killing and restarting the backend preserves all data (sessions survive).

## Out of scope for this sprint

- Personas, documents, chat, models, embeddings, style analysis, export (but `models.toml` and the integrity loader are wired up now).
- MFA / TOTP.
- Email verification on login (invite acceptance is proof enough).
- CSRF token endpoint — added in sprint 7 alongside mutation-heavy endpoints. Sprint-1 mutations are all cookie-authenticated POSTs over same-site, which SameSite=Lax already protects.
