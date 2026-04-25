-- Sprint 1 init migration: every table from docs/02-data-model.md
-- All schema additions from sprint 2+ are in separate migrations (append-only).
-- NOTE: session_index uses session_id_hash (sha3_256 of tower_sessions.id) per
--       the confirmed design (see docs/02-data-model.md and sprint-01-foundation.md).

-- Extensions
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS citext;
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- =============================================================================
-- Identity & auth
-- =============================================================================

CREATE TABLE users (
    id                     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email                  CITEXT UNIQUE NOT NULL,
    password_hash          TEXT NOT NULL,
    role                   TEXT NOT NULL CHECK (role IN ('admin', 'user')) DEFAULT 'user',
    status                 TEXT NOT NULL CHECK (status IN ('active', 'disabled')) DEFAULT 'active',
    display_name           TEXT,
    quota_storage_bytes    BIGINT NOT NULL DEFAULT 10737418240,
    current_storage_bytes  BIGINT NOT NULL DEFAULT 0,
    quota_doc_count        INT NOT NULL DEFAULT 5000,
    current_doc_count      INT NOT NULL DEFAULT 0,
    quota_persona_count    INT NOT NULL DEFAULT 50,
    current_persona_count  INT NOT NULL DEFAULT 0,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_login_at          TIMESTAMPTZ
);

-- Shadow projection of tower-sessions for "log out everywhere".
-- session_id_hash = sha3_256(tower_sessions.id) encoded as lowercase hex.
-- session_id      = raw tower_sessions.id (used for efficient O(n_user) revoke-all).
-- We store the raw ID alongside the hash so revoke-all can DELETE FROM tower_sessions
-- WHERE id = ANY(session_ids) without scanning the full tower_sessions table.
-- We do NOT add a FK to tower_sessions because that table is library-owned.
CREATE TABLE session_index (
    session_id_hash TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    ip              INET,
    user_agent      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at      TIMESTAMPTZ NOT NULL
);
CREATE INDEX session_index_user_id_idx ON session_index (user_id);
CREATE INDEX session_index_expires_at_idx ON session_index (expires_at);

CREATE TABLE login_attempts (
    id           BIGSERIAL PRIMARY KEY,
    email        CITEXT NOT NULL,
    ip           INET,
    success      BOOLEAN NOT NULL,
    attempted_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX login_attempts_email_idx ON login_attempts (email, attempted_at DESC);

-- Invite tokens: plaintext shown once; stored as sha3_256(plaintext) hex.
CREATE TABLE invite_tokens (
    token_hash  TEXT PRIMARY KEY,
    email       CITEXT NOT NULL,
    role        TEXT NOT NULL CHECK (role IN ('admin', 'user')) DEFAULT 'user',
    created_by  UUID NOT NULL REFERENCES users(id),
    expires_at  TIMESTAMPTZ NOT NULL,
    used_at     TIMESTAMPTZ,
    used_by     UUID REFERENCES users(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- At most one active (unused, unexpired) invite per email.
-- Enforces invite_pending 409 before a second invite can be created.
CREATE UNIQUE INDEX invite_tokens_active_email_uniq
    ON invite_tokens (email)
    WHERE used_at IS NULL AND expires_at > now();

-- Password reset tokens: same shape as invite tokens, TTL 30 min.
CREATE TABLE password_resets (
    token_hash  TEXT PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at  TIMESTAMPTZ NOT NULL,
    used_at     TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE audit_log (
    id            BIGSERIAL PRIMARY KEY,
    user_id       UUID REFERENCES users(id),
    action        TEXT NOT NULL,
    resource_type TEXT,
    resource_id   TEXT,
    ip            INET,
    metadata      JSONB,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX audit_log_user_id_idx ON audit_log (user_id, created_at DESC);

-- =============================================================================
-- Domain
-- =============================================================================

CREATE TABLE personas (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    relation    TEXT,
    description TEXT,
    avatar_path TEXT,
    birth_year  INT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, name)
);
CREATE INDEX personas_user_id_idx ON personas (user_id);

CREATE TABLE eras (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    persona_id  UUID NOT NULL REFERENCES personas(id) ON DELETE CASCADE,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    label       TEXT NOT NULL,
    start_date  DATE,
    end_date    DATE,
    description TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (persona_id, label)
);
CREATE INDEX eras_persona_id_idx ON eras (persona_id);

CREATE TABLE documents (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    persona_id      UUID NOT NULL REFERENCES personas(id) ON DELETE CASCADE,
    era_id          UUID REFERENCES eras(id) ON DELETE SET NULL,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL CHECK (kind IN ('text', 'audio')),
    mime_type       TEXT NOT NULL,
    original_path   TEXT NOT NULL,
    transcript_path TEXT,
    content_hash    TEXT NOT NULL,
    size_bytes      BIGINT NOT NULL,
    title           TEXT,
    source          TEXT,
    word_count      INT,
    duration_sec    INT,
    progress_pct    SMALLINT,
    status          TEXT NOT NULL CHECK (status IN (
                        'pending','parsing','transcribing','chunking',
                        'embedding','analysing','done','failed')
                    ) DEFAULT 'pending',
    error           TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    ingested_at     TIMESTAMPTZ
);
CREATE INDEX documents_persona_status_idx ON documents (persona_id, status);
CREATE INDEX documents_user_id_idx ON documents (user_id);
CREATE UNIQUE INDEX documents_persona_content_uniq ON documents (persona_id, content_hash);

CREATE TABLE chunks (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    document_id UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    persona_id  UUID NOT NULL REFERENCES personas(id) ON DELETE CASCADE,
    era_id      UUID REFERENCES eras(id) ON DELETE SET NULL,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    chunk_index INT NOT NULL,
    text        TEXT NOT NULL,
    token_count INT NOT NULL,
    embedding   vector(384),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX chunks_embedding_idx
    ON chunks USING hnsw (embedding vector_cosine_ops)
    WITH (m = 16, ef_construction = 64);
CREATE INDEX chunks_text_trgm_idx ON chunks USING gin (text gin_trgm_ops);
CREATE INDEX chunks_persona_era_idx ON chunks (persona_id, era_id);
CREATE INDEX chunks_user_id_idx ON chunks (user_id);
CREATE INDEX chunks_doc_idx ON chunks (document_id, chunk_index);

-- =============================================================================
-- Style profiles
-- =============================================================================

CREATE TABLE style_profiles (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    persona_id    UUID NOT NULL REFERENCES personas(id) ON DELETE CASCADE,
    era_id        UUID REFERENCES eras(id) ON DELETE CASCADE,
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    corpus_tokens INT NOT NULL,
    profile       JSONB NOT NULL,
    computed_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- At most one whole-persona profile (era_id IS NULL per persona)
CREATE UNIQUE INDEX style_profiles_persona_null_era_uniq
    ON style_profiles (persona_id)
    WHERE era_id IS NULL;
-- At most one profile per (persona, era)
CREATE UNIQUE INDEX style_profiles_persona_era_uniq
    ON style_profiles (persona_id, era_id)
    WHERE era_id IS NOT NULL;

-- =============================================================================
-- Chat
-- =============================================================================

CREATE TABLE chat_sessions (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    persona_id  UUID NOT NULL REFERENCES personas(id) ON DELETE CASCADE,
    era_id      UUID REFERENCES eras(id) ON DELETE SET NULL,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title       TEXT,
    model_id    TEXT NOT NULL,
    temperature REAL NOT NULL DEFAULT 0.7,
    top_p       REAL NOT NULL DEFAULT 0.9,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX chat_sessions_persona_idx ON chat_sessions (persona_id, created_at DESC);
CREATE INDEX chat_sessions_user_id_idx ON chat_sessions (user_id);

CREATE TABLE messages (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id          UUID NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    user_id             UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role                TEXT NOT NULL CHECK (role IN ('system', 'user', 'assistant')),
    content             TEXT NOT NULL,
    retrieved_chunk_ids UUID[] DEFAULT '{}',
    tokens_in           INT,
    tokens_out          INT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX messages_session_idx ON messages (session_id, created_at);

-- =============================================================================
-- Background jobs
-- =============================================================================

CREATE TABLE jobs (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kind         TEXT NOT NULL,
    user_id      UUID REFERENCES users(id) ON DELETE CASCADE,
    persona_id   UUID REFERENCES personas(id) ON DELETE CASCADE,
    payload      JSONB NOT NULL,
    status       TEXT NOT NULL CHECK (status IN ('queued','running','done','failed')) DEFAULT 'queued',
    attempts     INT NOT NULL DEFAULT 0,
    worker_id    TEXT,
    heartbeat_at TIMESTAMPTZ,
    last_error   TEXT,
    scheduled_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at   TIMESTAMPTZ,
    finished_at  TIMESTAMPTZ
);
CREATE INDEX jobs_status_scheduled_idx ON jobs (status, scheduled_at);
CREATE INDEX jobs_user_id_status_idx ON jobs (user_id, status);
CREATE INDEX jobs_persona_id_status_idx ON jobs (persona_id, status);

-- =============================================================================
-- Idempotency
-- =============================================================================

CREATE TABLE idempotency_keys (
    key             TEXT NOT NULL,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    route           TEXT NOT NULL,
    request_hash    TEXT NOT NULL,
    response_status INT NOT NULL,
    response_body   JSONB,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, route, key)
);
CREATE INDEX idempotency_keys_created_at_idx ON idempotency_keys (created_at);

-- =============================================================================
-- Errors (5xx captured by middleware; 30-day retention)
-- =============================================================================

CREATE TABLE errors (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    UUID REFERENCES users(id) ON DELETE SET NULL,
    route      TEXT,
    code       TEXT NOT NULL,
    message    TEXT NOT NULL,
    backtrace  TEXT,
    request_id TEXT,
    ip         INET,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX errors_created_at_idx ON errors (created_at DESC);
CREATE INDEX errors_user_id_idx ON errors (user_id, created_at DESC);
