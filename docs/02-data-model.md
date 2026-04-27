# Data model

Single Postgres 16 database with the `pgvector` and `pg_trgm` extensions. All tables scoped by `user_id` at the row level; application-layer filters enforce isolation.

## Extensions

```sql
CREATE EXTENSION IF NOT EXISTS vector;      -- pgvector for embeddings
CREATE EXTENSION IF NOT EXISTS pg_trgm;     -- trigram index for hybrid search
CREATE EXTENSION IF NOT EXISTS citext;      -- case-insensitive emails
CREATE EXTENSION IF NOT EXISTS "uuid-ossp"; -- uuid_generate_v4 (optional; gen_random_uuid works too)
```

## Entity overview

```
users ─┬─< tower_sessions (library-managed)
       ├─< invite_tokens (created_by)
       ├─< password_resets
       ├─< login_attempts
       ├─< audit_log
       ├─< provider_configs (per-service AI provider settings)
       └─< personas ─┬─< eras
                     ├─< documents ──< chunks (embedding vector)
                     ├─< style_profiles (one per persona, per era optional)
                     └─< chat_sessions ──< messages
```

`jobs`, `idempotency_keys`, and `errors` are system tables used by the worker / middleware.

## Schema

### Identity & auth

```sql
CREATE TABLE users (
    id                     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email                  CITEXT UNIQUE NOT NULL,
    password_hash          TEXT NOT NULL,
    role                   TEXT NOT NULL CHECK (role IN ('admin', 'user')) DEFAULT 'user',
    status                 TEXT NOT NULL CHECK (status IN ('active', 'disabled')) DEFAULT 'active',
    display_name           TEXT,
    quota_storage_bytes    BIGINT NOT NULL DEFAULT 10737418240,   -- 10 GB default
    current_storage_bytes  BIGINT NOT NULL DEFAULT 0,             -- maintained on doc insert/delete; nightly reconcile
    quota_doc_count        INT NOT NULL DEFAULT 5000,             -- per-user document cap
    current_doc_count      INT NOT NULL DEFAULT 0,                -- maintained on doc insert/delete; nightly reconcile
    quota_persona_count    INT NOT NULL DEFAULT 50,               -- per-user persona cap
    current_persona_count  INT NOT NULL DEFAULT 0,                -- maintained on persona insert/delete; nightly reconcile
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_login_at          TIMESTAMPTZ
);
-- Quota counters are eventually consistent: the upload / create handler increments
-- atomically in the same transaction as the insert, and a nightly reconcile job
-- recomputes them from ground truth (COUNT(*), SUM(size_bytes)).

-- Sessions table is owned by `tower-sessions-sqlx-store`. It creates and manages:
--   CREATE TABLE tower_sessions (
--     id          TEXT PRIMARY KEY,
--     data        BYTEA NOT NULL,
--     expires_at  TIMESTAMPTZ NOT NULL
--   );
-- The `data` BYTEA is a serialised key-value map; we store `user_id`, `ip`, `user_agent`,
-- `created_at`, and `csrf_token` inside that blob. We do not add columns to this table —
-- the library would not populate them, and that creates confusion.
--
-- If we need to query "all sessions for user X" (for revoke-all), we shadow the library's
-- table with an application-managed projection:
--
-- Sprint 1 decision: we store sha3_256(tower_sessions.id) rather than the raw session ID.
-- Rationale: the raw session ID is equivalent to a Bearer token. Hashing it means the
-- session_index table is safe to inspect in DB tooling without leaking live credentials.
-- Revoke-all loads all session_index hashes for a user, loads all tower_sessions IDs from
-- the library table, computes sha3_256 in Rust to find matches, then deletes both rows.
-- Postgres's pgcrypto does not implement SHA-3, so the join is done in application code.
CREATE TABLE session_index (
    session_id_hash TEXT PRIMARY KEY,       -- sha3_256(tower_sessions.id) hex-encoded
    session_id      TEXT NOT NULL,          -- raw tower_sessions.id for O(n_user) revoke-all
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    ip              INET,
    user_agent      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at      TIMESTAMPTZ NOT NULL
);
CREATE INDEX ON session_index (user_id);
CREATE INDEX ON session_index (expires_at);
-- Populated in the login handler; deleted on logout; nightly sweep removes rows whose
-- session_id_hash no longer has a matching tower_sessions row.

CREATE TABLE login_attempts (
    id          BIGSERIAL PRIMARY KEY,
    email       CITEXT NOT NULL,
    ip          INET,
    success     BOOLEAN NOT NULL,
    attempted_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ON login_attempts (email, attempted_at DESC);
-- Used for per-account brute-force throttle. Pruned with audit retention.

CREATE TABLE invite_tokens (
    token_hash      TEXT PRIMARY KEY,                 -- sha3_256 of token; plaintext shown once
    email           CITEXT NOT NULL,
    role            TEXT NOT NULL CHECK (role IN ('admin', 'user')) DEFAULT 'user',
    created_by      UUID NOT NULL REFERENCES users(id),
    expires_at      TIMESTAMPTZ NOT NULL,
    used_at         TIMESTAMPTZ,
    used_by         UUID REFERENCES users(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- Only one active (unused, not expired) invite per email:
CREATE UNIQUE INDEX invite_tokens_active_email_uniq
    ON invite_tokens (email)
    WHERE used_at IS NULL AND expires_at > now();
-- Re-issuing an invite requires revoking the existing one. Inviting an email that already
-- has an active user is rejected by the service layer (409 user_exists).

CREATE TABLE password_resets (
    token_hash      TEXT PRIMARY KEY,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at      TIMESTAMPTZ NOT NULL,
    used_at         TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE audit_log (
    id              BIGSERIAL PRIMARY KEY,
    user_id         UUID REFERENCES users(id),
    action          TEXT NOT NULL,                   -- e.g. 'invite.created', 'persona.deleted'
    resource_type   TEXT,
    resource_id     TEXT,
    ip              INET,
    metadata        JSONB,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ON audit_log (user_id, created_at DESC);
```

### Domain

```sql
CREATE TABLE personas (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,                   -- "Me, age 15"
    relation        TEXT,                            -- 'self', 'family', 'friend', 'other'
    description     TEXT,
    avatar_path     TEXT,                            -- relative to /data
    birth_year      INT,                             -- optional anchor for 'age X' eras
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, name)
);
CREATE INDEX ON personas (user_id);

CREATE TABLE eras (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    persona_id      UUID NOT NULL REFERENCES personas(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,   -- denormalised for filter perf
    label           TEXT NOT NULL,                   -- "age 13–16"
    start_date      DATE,
    end_date        DATE,
    description     TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (persona_id, label)
);
CREATE INDEX ON eras (persona_id);

CREATE TABLE documents (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    persona_id      UUID NOT NULL REFERENCES personas(id) ON DELETE CASCADE,
    era_id          UUID REFERENCES eras(id) ON DELETE SET NULL,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL CHECK (kind IN ('text', 'audio')),
    mime_type       TEXT NOT NULL,
    original_path   TEXT NOT NULL,                   -- /data/uploads/<persona_id>/<doc_id>.<ext>
    transcript_path TEXT,                            -- for audio
    content_hash    TEXT NOT NULL,                   -- sha256 hex of original bytes
    size_bytes      BIGINT NOT NULL,                 -- for quota accounting
    title           TEXT,
    source          TEXT,                            -- free-form: "journal 2010", "interview"
    word_count      INT,
    duration_sec    INT,                             -- for audio
    progress_pct    SMALLINT,                        -- 0-100 during transcribing; NULL otherwise
    status          TEXT NOT NULL CHECK (status IN (
                        'pending','parsing','transcribing','chunking','embedding','analysing','done','failed')
                     ) DEFAULT 'pending',
    detected_language TEXT,                            -- BCP-47 code; set after ingestion (lingua for text, Whisper for audio)
    error           TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    ingested_at     TIMESTAMPTZ
);
CREATE INDEX ON documents (persona_id, status);
CREATE INDEX ON documents (user_id);
-- Duplicate detection within a persona: reject re-upload of the same file bytes.
CREATE UNIQUE INDEX documents_persona_content_uniq
    ON documents (persona_id, content_hash);

CREATE TABLE chunks (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    document_id     UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    persona_id      UUID NOT NULL REFERENCES personas(id) ON DELETE CASCADE,
    era_id          UUID REFERENCES eras(id) ON DELETE SET NULL,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    chunk_index     INT NOT NULL,
    text            TEXT NOT NULL,
    token_count     INT NOT NULL,
    embedding       vector(1024),                    -- BAAI/bge-m3; nullable during ingestion, populated by embed phase
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX chunks_embedding_idx
    ON chunks USING hnsw (embedding vector_cosine_ops)
    WITH (m = 16, ef_construction = 64);

CREATE INDEX chunks_text_trgm_idx
    ON chunks USING gin (text gin_trgm_ops);

CREATE INDEX ON chunks (persona_id, era_id);
CREATE INDEX ON chunks (user_id);
CREATE INDEX ON chunks (document_id, chunk_index);
```

### Style profiles

A single JSONB document per persona (and optionally per era). Rebuilt on ingestion.

```sql
CREATE TABLE style_profiles (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    persona_id      UUID NOT NULL REFERENCES personas(id) ON DELETE CASCADE,
    era_id          UUID REFERENCES eras(id) ON DELETE CASCADE,     -- NULL = whole persona
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    corpus_tokens   INT NOT NULL,
    profile         JSONB NOT NULL,                                 -- see sprint-04 for schema
    computed_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- Two partial unique indexes: at most one whole-persona profile (era_id IS NULL) and
-- at most one profile per (persona, era). A plain UNIQUE (persona_id, era_id) is wrong
-- because Postgres treats each NULL as distinct, allowing duplicates when era_id IS NULL.
CREATE UNIQUE INDEX style_profiles_persona_null_era_uniq
    ON style_profiles (persona_id)
    WHERE era_id IS NULL;
CREATE UNIQUE INDEX style_profiles_persona_era_uniq
    ON style_profiles (persona_id, era_id)
    WHERE era_id IS NOT NULL;
```

The `profile` JSON shape is specified in [`sprints/sprint-04-analysis.md`](sprints/sprint-04-analysis.md).

### Chat

```sql
CREATE TABLE chat_sessions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    persona_id      UUID NOT NULL REFERENCES personas(id) ON DELETE CASCADE,
    era_id          UUID REFERENCES eras(id) ON DELETE SET NULL,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title           TEXT,
    model_id        TEXT NOT NULL,                   -- e.g. 'qwen2.5-7b-instruct-q4_k_m'
    temperature     REAL NOT NULL DEFAULT 0.7,
    top_p           REAL NOT NULL DEFAULT 0.9,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ON chat_sessions (persona_id, created_at DESC);
CREATE INDEX ON chat_sessions (user_id);

CREATE TABLE messages (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id      UUID NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role            TEXT NOT NULL CHECK (role IN ('system', 'user', 'assistant')),
    content         TEXT NOT NULL,
    retrieved_chunk_ids UUID[] DEFAULT '{}',         -- citations for assistant msgs
    tokens_in       INT,
    tokens_out      INT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ON messages (session_id, created_at);
```

### Background jobs

Persisted so the worker can resume after a crash.

```sql
CREATE TABLE jobs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kind            TEXT NOT NULL,                   -- 'ingest_document', 'recompute_profile',
                                                     --   'storage_cleanup', 'user_export', 'user_delete'
    user_id         UUID REFERENCES users(id) ON DELETE CASCADE,  -- owner, NULL only for system jobs
    persona_id      UUID REFERENCES personas(id) ON DELETE CASCADE, -- scoping hint for reaper/cancel
    payload         JSONB NOT NULL,
    status          TEXT NOT NULL CHECK (status IN (
                        'queued','running','done','failed')
                     ) DEFAULT 'queued',
    attempts        INT NOT NULL DEFAULT 0,
    worker_id       TEXT,                            -- heartbeat owner; NULL when not running
    heartbeat_at    TIMESTAMPTZ,                     -- updated every 30s while running
    last_error      TEXT,
    scheduled_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at      TIMESTAMPTZ,
    finished_at     TIMESTAMPTZ
);
CREATE INDEX ON jobs (status, scheduled_at);
CREATE INDEX ON jobs (user_id, status);
CREATE INDEX ON jobs (persona_id, status);
-- Stuck-job reaper (on startup and every minute): any row with status='running' and
-- heartbeat_at < now() - interval '2 minutes' is reset to 'queued' with attempts+1,
-- worker_id cleared. started_at stays for audit.
```

### Idempotency

```sql
CREATE TABLE idempotency_keys (
    key              TEXT NOT NULL,
    user_id          UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    route            TEXT NOT NULL,
    request_hash     TEXT NOT NULL,                  -- sha256 of canonicalised request body
    response_status  INT NOT NULL,
    response_body    JSONB,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, route, key)
);
CREATE INDEX ON idempotency_keys (created_at);
-- Pruned daily; rows > 24h old deleted.
```

### Provider configs

Per-user AI provider configurations. Local providers (priority 0) are inserted automatically on user creation and cannot be deleted, only disabled. Cloud providers are user-managed.

```sql
CREATE TABLE provider_configs (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    service     TEXT NOT NULL CHECK (service IN ('transcription', 'llm', 'embeddings')),
    provider    TEXT NOT NULL,
    -- local:  'local_whisper' | 'local_llama' | 'local_bge'
    -- cloud:  'openai_compat' | 'google_speech'
    priority    INT  NOT NULL DEFAULT 10,
    -- lower number = tried first; local providers default to priority 0
    config      JSONB NOT NULL DEFAULT '{}',
    -- sensitive fields (api_key, endpoint) stored AES-256-GCM encrypted as {"enc": "<base64>"}
    -- non-sensitive fields stored plaintext: {"model": "gpt-4o-mini", ...}
    enabled     BOOL NOT NULL DEFAULT true,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, service, provider)
);
CREATE INDEX ON provider_configs (user_id, service, priority) WHERE enabled;
```

API key encryption uses AES-256-GCM; the 32-byte key is derived from `SESSION_SECRET` via `HKDF-SHA256(secret, salt="provider-key-v1", len=32)`. Rotating `SESSION_SECRET` invalidates all stored API keys — see runbook §12.

### Errors

Captured at the middleware layer for admin investigation.

```sql
CREATE TABLE errors (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       UUID REFERENCES users(id) ON DELETE SET NULL,
    route         TEXT,
    code          TEXT NOT NULL,
    message       TEXT NOT NULL,
    backtrace     TEXT,
    request_id    TEXT,
    ip            INET,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ON errors (created_at DESC);
CREATE INDEX ON errors (user_id, created_at DESC);
-- 30-day retention; pruned nightly.
```

## Cascade map

What happens when each top-level row is deleted. All cascades are via `ON DELETE CASCADE` FKs unless noted.

| Deleting… | Cascades to |
|-----------|-------------|
| `users` | `personas`, `eras`, `documents`, `chunks`, `style_profiles`, `chat_sessions`, `messages`, `session_index`, `password_resets`, `audit_log` (nullifies `user_id`; keeps row), `errors` (nullifies `user_id`; keeps row), `idempotency_keys`, `jobs` (where `jobs.user_id = users.id`), `provider_configs`. `login_attempts` and `invite_tokens` rows remain for audit. Filesystem cleanup (uploads / transcripts / avatars / exports) runs in a follow-up job. |
| `personas` | `eras`, `documents` → `chunks`, `style_profiles`, `chat_sessions` → `messages`, `jobs` (where `jobs.persona_id = personas.id`). Filesystem: `/data/uploads/<persona_id>/`, `/data/transcripts/<doc_id>.txt` for its docs, `/data/avatars/<persona_id>.*` cleaned in a follow-up job. |
| `eras` | `era_id` is set to NULL on dependents (`documents.era_id`, `chunks.era_id`, `chat_sessions.era_id`), **except** `style_profiles(era_id)` which cascades (profiles are per-era and meaningless without the era). |
| `documents` | `chunks`. Filesystem cleanup for `original_path` + `transcript_path` in a follow-up job. |
| `chat_sessions` | `messages`. |

Cross-user deletion never happens in the schema because no FK crosses `user_id` boundaries. The `user_id` denormalisation on `eras`, `documents`, `chunks`, `chat_sessions`, `messages`, `style_profiles`, `jobs` exists for query-time filter efficiency, not cascade correctness.

## Filesystem layout (for binary content)

```
/data/uploads/<persona_id>/<document_id>.<ext>      # originals
/data/transcripts/<document_id>.txt                 # whisper transcripts
/data/avatars/<persona_id>.<ext>                    # persona avatars
/data/exports/<user_id>/<session_id>-<ts>.docx      # generated exports (ephemeral)
```

`document_id` is a UUID so paths don't leak ordering. All file writes go through a `Storage` trait so we can swap in S3 later if needed.

## Authorization invariant

Every SELECT, UPDATE, DELETE on a domain table includes `user_id = $current_user`. This is enforced by:

1. Repository functions take `user_id: UserId` as a typed parameter (not optional, not defaulted).
2. There is no raw SQL in handlers; all DB access goes through the repositories.
3. A smoke-test migration seeds two users and a unit test asserts user A cannot read user B's rows via any public API.

## Migration strategy

- `sqlx migrate add <name>` to create timestamped migration files in `backend/migrations/`.
- `sqlx migrate run` on startup in dev; explicit step in prod Dockerfile.
- **All schema in this doc lives in the initial migration** (`20260425000000_init.sql`). Later migrations only add columns, indexes, or tables for features added after the initial release. Do not split the initial schema across sprints.
- Every migration ships with a `down.sql` counterpart that undoes it. Down migrations are not run automatically but document intent and are referenced during incident response.
- No destructive migrations without a backup note in the migration file header.
- The date `20260425` is today's date (2026-04-25), not a typo; migrations use the date they are authored.
