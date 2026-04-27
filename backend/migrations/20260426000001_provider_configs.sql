-- Sprint 8: provider configs + detected language column

-- Provider configurations table (§8.1)
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
    -- sensitive fields (api_key, endpoint) are AES-256-GCM encrypted with
    -- a key derived from app config; stored as {"enc": "<base64-ciphertext>"}
    -- non-sensitive fields stored plaintext: {"model": "gpt-4o-mini", ...}
    enabled     BOOL NOT NULL DEFAULT true,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, service, provider)
);
CREATE INDEX ON provider_configs (user_id, service, priority) WHERE enabled;

-- Detected language for documents (§8.10)
ALTER TABLE documents ADD COLUMN IF NOT EXISTS detected_language TEXT;
