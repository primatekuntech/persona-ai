# Sprint 8 — Multi-language Support & Provider Abstraction

**Goal:** the app works well for Malaysian users whose content spans Bahasa Malaysia,
English, Mandarin, and Cantonese. All AI services (transcription, LLM, embeddings) sit
behind a provider trait; local models are the default with zero external dependencies.
Users can opt-in to cloud APIs via an Integrations settings page.

**Duration estimate:** 5–7 days.

## Background

Malaysia's linguistic landscape requires:
- **Written:** Bahasa Malaysia, English, Simplified/Traditional Chinese, Tamil, colloquial Cantonese text
- **Spoken:** same set + Cantonese distinctly different from Mandarin at the phoneme level

The existing stack handles English well. This sprint upgrades every AI touchpoint to be
language-aware and adds an extensibility layer so cloud providers can be added without
changing business logic.

## Deliverables

1. `provider_configs` table + repository + CRUD routes.
2. Provider trait layer (`TranscriptionProvider`, `LlmProvider`, `EmbeddingProvider`).
3. Existing local models (Whisper, llama.cpp, fastembed) re-wired through the traits.
4. Language detection for text documents (`lingua` crate) and audio (Whisper auto-detect).
5. Mandarin / Cantonese routing: pass `zh` vs `yue` language hint to Whisper.
6. Upgrade embedding model default from `bge-small-en` to `bge-m3` (multilingual, 100+ langs).
7. `OpenAICompatProvider` for LLM — covers OpenAI, Together, Ollama, LM Studio, any
   OpenAI-compatible endpoint. API key stored encrypted at rest.
8. `GoogleSpeechProvider` for transcription — covers Cantonese (`yue-HK`) better than
   local Whisper.
9. Settings → Integrations tab in the frontend: add/edit/delete/test provider configs.
10. Masked API key display (never returned in full after save).

## Data model additions

### 8.1 `provider_configs`

```sql
CREATE TABLE provider_configs (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    service     TEXT NOT NULL CHECK (service IN ('transcription', 'llm', 'embeddings')),
    provider    TEXT NOT NULL,
    -- local:  'local_whisper' | 'local_llama' | 'local_bge'
    -- cloud:  'openai_compat' | 'google_speech' | 'azure_speech'
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
```

Local provider rows are inserted automatically on user creation (priority 0, config `{}`).
They cannot be deleted, only disabled.

## Backend tasks

### 8.2 Provider traits

```
src/services/providers/
  mod.rs          — re-exports, ProviderRegistry, Language enum, LanguageHint
  transcription.rs — TranscriptionProvider trait + LocalWhisperProvider + GoogleSpeechProvider
  llm.rs          — LlmProvider trait + LocalLlamaProvider + OpenAICompatProvider
  embeddings.rs   — EmbeddingProvider trait + LocalBgeProvider
  encrypt.rs      — AES-256-GCM helpers for api_key storage
```

**`Language` enum (non-exhaustive):**

```rust
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Language {
    English,
    Malay,
    MandarinSimplified,
    MandarinTraditional,
    Cantonese,
    Tamil,
    Other(String),   // BCP-47 code e.g. "iba" (Iban)
}

impl Language {
    /// BCP-47 / Whisper language code
    pub fn whisper_code(&self) -> &str { ... }
    /// Google Speech BCP-47 code
    pub fn google_code(&self) -> &str { ... }
}
```

**`TranscriptionProvider` trait:**

```rust
#[async_trait]
pub trait TranscriptionProvider: Send + Sync {
    fn name(&self) -> &str;
    fn supports_language(&self, lang: &Language) -> bool;
    async fn transcribe(
        &self,
        audio_path: &Path,
        hint: LanguageHint,
    ) -> Result<Transcript>;
}

pub struct LanguageHint {
    pub detected: Option<Language>,   // from prior text analysis
    pub user_override: Option<Language>,
}

pub struct Transcript {
    pub text: String,
    pub detected_language: Option<Language>,
    pub confidence: Option<f32>,
    pub segments: Vec<TranscriptSegment>,
}
```

**`LlmProvider` trait:**

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn generate(
        &self,
        req: GenerateRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>>;
}
```

**`ProviderRegistry`** lives in `AppState`:

```rust
pub struct ProviderRegistry {
    pub transcription: HashMap<String, Arc<dyn TranscriptionProvider>>,
    pub llm:           HashMap<String, Arc<dyn LlmProvider>>,
    pub embeddings:    HashMap<String, Arc<dyn EmbeddingProvider>>,
}

impl ProviderRegistry {
    /// Returns enabled providers for this user ordered by priority (lowest first).
    pub async fn transcription_chain(
        &self,
        user_id: Uuid,
        db: &PgPool,
    ) -> Result<Vec<Arc<dyn TranscriptionProvider>>>;
    // similar for llm_chain, embeddings_chain
}
```

Callers iterate the chain and try the first provider. On error they log the failure to
the `errors` table and try the next. If all fail, the job itself fails.

### 8.3 Language detection

Add `lingua` to `Cargo.toml` (`lingua = { version = "1", features = ["english", "malay",
"chinese", "cantonese", "tamil"] }`). Run in `spawn_blocking`.

```rust
pub fn detect_language(text: &str) -> Option<Language> { ... }
```

Called in the ingestion pipeline (after text extraction, before chunking) and stored in
`documents.detected_language TEXT` (new column). Cantonese detection uses script + character
frequency heuristics because most CJK models conflate it with Mandarin.

For **audio**, Whisper reports `detected_language` in its output. The worker stores it back
to `documents.detected_language` after transcription.

### 8.4 Language routing in transcription

`LocalWhisperProvider::transcribe` sets `language` parameter:

```rust
let lang_code = hint.user_override
    .or(hint.detected)
    .map(|l| l.whisper_code())
    .unwrap_or("auto");  // Whisper auto-detect
```

Whisper large-v3 supports `yue` (Cantonese). For users who have Google Speech configured at
a higher priority and `detected_language = Cantonese`, Google Speech is tried first.

### 8.5 Embedding model upgrade

Replace `bge-small-en-v1.5` default with `BAAI/bge-m3`. Change in
`src/services/worker.rs` (or wherever `TextEmbeddingModel::new` is called).

`bge-m3` supports 100+ languages including all Malaysian languages. The model is
~570 MB vs ~130 MB for `bge-small-en`. Embeddings dimension stays 768 (compatible
with existing pgvector column — no migration needed if dimension matches; if existing
column is a different size, migration alters it).

**Note:** existing embeddings generated with `bge-small-en` are incompatible with
`bge-m3`. On upgrade, set `documents.embedding_status = 'pending_reembed'` and re-run
the embed phase for all documents. A one-off migration job handles this.

### 8.6 `OpenAICompatProvider`

Calls any OpenAI-compatible `/v1/chat/completions` endpoint with streaming. Config fields:

```json
{
  "endpoint": "https://api.openai.com",
  "model": "gpt-4o-mini",
  "enc_api_key": "<AES-GCM ciphertext base64>"
}
```

Works with OpenAI, Together AI, Anyscale, Ollama (`http://localhost:11434`), LM Studio.
Streams `delta.content` tokens via SSE, maps to the same `Stream<Item=Result<String>>`
as the local provider.

### 8.7 `GoogleSpeechProvider`

Calls Google Cloud Speech-to-Text v2 REST API. Config:

```json
{
  "enc_api_key": "<base64>",
  "region": "global"
}
```

Language map:
- Mandarin → `cmn-Hans-CN` or `cmn-Hant-TW`
- Cantonese → `yue-Hant-HK`
- Malay → `ms-MY`
- English → `en-MY` (Malaysian English model)
- Tamil → `ta-MY`

### 8.8 API key encryption

AES-256-GCM using a 32-byte key derived from `config.session_secret` via
`HKDF-SHA256(secret, salt="provider-key-v1", len=32)`. Nonce is random per encrypt,
prepended to ciphertext: `nonce(12) || ciphertext`.

```rust
// src/services/providers/encrypt.rs
pub fn encrypt_api_key(plaintext: &str, app_secret: &str) -> Result<String>;
pub fn decrypt_api_key(ciphertext_b64: &str, app_secret: &str) -> Result<String>;
```

Rotating `session_secret` invalidates all stored API keys (users must re-enter). Document
this in the runbook.

### 8.9 Provider config routes

```
GET    /api/providers                    → list user's configs (api_key masked)
POST   /api/providers                    → add config (201)
PATCH  /api/providers/:id                → update config (partial)
DELETE /api/providers/:id                → delete (local providers return 409 cannot_delete)
POST   /api/providers/:id/test           → test connectivity (200 ok / 422 failed)
```

**`POST /api/providers` request:**
```json
{
  "service":  "llm",
  "provider": "openai_compat",
  "priority": 5,
  "config": {
    "endpoint":  "https://api.openai.com",
    "model":     "gpt-4o-mini",
    "api_key":   "sk-..."
  }
}
```

`api_key` is encrypted server-side before storage. Response replaces `api_key` with
`"api_key_hint": "sk-...xxxx"` (last 4 chars). Subsequent GETs also return only the hint.

**`POST /api/providers/:id/test`** fires a minimal probe:
- LLM: `POST /v1/chat/completions` with `{"model": "...", "messages": [{"role":"user","content":"ping"}], "max_tokens": 1}`
- Transcription: `POST` with 1-second silence audio
- Returns `{"ok": true}` or `{"ok": false, "error": "..."}`

### 8.10 Documents table addition

```sql
ALTER TABLE documents ADD COLUMN IF NOT EXISTS detected_language TEXT;
```

Populated after ingestion (text via lingua, audio via Whisper output).

## Frontend tasks

### 8.11 Settings → Integrations tab

New tab in `/settings/account` (or separate route `/settings/integrations`).

Three sections, one per service:

**Transcription**
```
[✓] Local — Whisper large-v3        priority 0  (always on)
[ ] Google Speech-to-Text           [+ Add]
```

**Language model**
```
[✓] Local — Qwen2.5 / SeaLLM       priority 0  (always on)
[ ] OpenAI-compatible endpoint      [+ Add]
```

**Embeddings** (collapsed by default — advanced)
```
[✓] Local — bge-m3                  priority 0  (always on)
```

Clicking **+ Add** opens a dialog with fields appropriate to the provider type.
After saving, a **Test** button confirms connectivity.

Priority can be adjusted (↑↓ or drag) — lower priority number = tried first.
Local providers are pinned at 0 and cannot be removed.

### 8.12 API key field behaviour

- On save: field cleared, replaced with placeholder showing `sk-...xxxx`.
- On edit: blank field = "keep existing key"; non-blank = "replace key".
- Never pre-filled from the server.

## Acceptance tests

- [ ] Uploading a Mandarin `.txt` document: `detected_language = 'zh-Hans'` after ingestion.
- [ ] Uploading a Cantonese voice note: transcribed with `yue` hint, `detected_language = 'yue'`.
- [ ] Uploading a Malay `.txt`: embeddings stored, RAG retrieval returns relevant chunks.
- [ ] Adding an OpenAI-compatible config, test button returns `{"ok": true}`.
- [ ] Chat with OpenAI provider enabled: tokens stream correctly.
- [ ] Disabling the OpenAI provider: chat falls back to local LLM without error.
- [ ] Deleting an OpenAI config: subsequent requests use local LLM only.
- [x] Attempting to delete a local provider: `409 cannot_delete`.
- [x] API key never returned in full from any GET endpoint.
- [x] `cargo clippy -- -D warnings` clean.
- [x] `tsc --noEmit` clean.

## Dependencies added this sprint

```toml
# Language detection
lingua = { version = "1", default-features = false, features = [
    "english", "malay", "chinese", "tamil"
] }
# HKDF for key derivation
hkdf = "0.12"
# AES-GCM encryption
aes-gcm = "0.10"
```

Frontend: no new dependencies (uses existing `react-hook-form` + `zod` for provider config forms).
