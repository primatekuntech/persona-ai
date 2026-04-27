# Architecture

## System diagram

```
┌───────────────────────────────────────┐     ┌───────────────────────────────────────┐
│  Frontend                             │     │  Backend (single Rust binary)         │
│  React 18 + Vite + TypeScript         │     │  axum + tokio                         │
│                                       │     │                                       │
│  Pages:                               │     │  Layers:                              │
│   • /login                            │─────│   1. HTTP (axum routes, SSE)          │
│   • /accept-invite                    │ SSE │   2. Middleware (auth, ratelimit,     │
│   • /personas (list + new)            │     │      tracing, CORS)                   │
│   • /personas/:id/dashboard           │     │   3. Services (persona, ingest,       │
│   • /personas/:id/upload              │     │      analysis, chat, export, admin)   │
│   • /personas/:id/chat                │     │   4. Workers (background tasks:       │
│   • /admin/users, /admin/invites      │     │      transcribe, chunk, embed,        │
│   • /settings/integrations            │     │      analyse)                         │
│                                       │     │   5. Repositories (sqlx → Postgres)   │
│  State: TanStack Query + Zustand      │     │   6. Model runtimes (whisper, llama,  │
│  UI: shadcn/ui + Tailwind             │     │      fastembed)                       │
└───────────────────────────────────────┘     │   7. Provider registry (local-first;  │
                                              │      cloud providers opt-in)          │
                                              └────────────────┬──────────────────────┘
                                                               │
                        ┌──────────────────────────────────────┼──────────────────────────────────────┐
                        ▼                                      ▼                                      ▼
              ┌──────────────────────┐            ┌──────────────────────┐            ┌──────────────────────┐
              │  Postgres 16         │            │  Filesystem          │            │  Model files         │
              │  + pgvector ext      │            │  /data/uploads/      │            │  /data/models/       │
              │                      │            │  /data/transcripts/  │            │   ggml-small.en.bin  │
              │  auth, personas,     │            │  /data/exports/      │            │   bge-small.onnx     │
              │  documents, chunks,  │            │                      │            │   qwen2.5-7b-q4.gguf │
              │  embeddings (HNSW),  │            │                      │            │                      │
              │  chats, audit        │            │                      │            │                      │
              └──────────────────────┘            └──────────────────────┘            └──────────────────────┘
```

Everything except static model files lives in a single Postgres database. This keeps backups to a single `pg_dump` + `rsync /data/uploads`.

## Tech stack (pinned versions targeted)

### Backend

| Area | Crate | Purpose |
|------|-------|---------|
| Runtime | `tokio` 1.x | Async runtime |
| HTTP | `axum` 0.7 | Router, extractors, SSE |
| Middleware | `tower-http` 0.5 | CORS, tracing, compression |
| Sessions | `tower-sessions` + `tower-sessions-sqlx-store` | Server-side sessions in Postgres |
| DB | `sqlx` 0.8 | Postgres driver, compile-time checked queries |
| Migrations | `sqlx-cli` | Versioned migrations |
| Password hashing | `argon2` | OWASP-current |
| Random | `rand` | Invite tokens, session ids |
| Email | `resend-rs` (or raw `reqwest` to Resend) | Invite + reset emails |
| Audio | `whisper-rs` | whisper.cpp bindings (Whisper large-v3, multilingual) |
| Embeddings | `fastembed` (crate) | BAAI/bge-m3 on CPU (100+ languages) |
| LLM | `llama-cpp-2` | llama.cpp bindings |
| Language detection | `lingua` | Text language detection for 100+ languages |
| Key derivation | `hkdf` | HKDF-SHA256 for provider API key encryption |
| Encryption | `aes-gcm` | AES-256-GCM for stored API keys |
| Async traits | `async-trait` | Provider trait objects |
| Text extraction | `docx-rs`, `lopdf`, `pulldown-cmark` | .docx, .pdf, .md parsing |
| Chunking | `text-splitter` | Sentence-aware chunker |
| Doc export | `docx-rs` | .docx writer |
| Errors | `thiserror`, `anyhow` | Library / binary error handling |
| Logging | `tracing`, `tracing-subscriber` | Structured logs |
| Config | `figment` | TOML + env |
| HTTP client | `reqwest` | Resend + healthchecks |
| Rate limit | `tower-governor` | Per-IP limiter for /auth |
| UUID | `uuid` 1.x | IDs |
| JSON | `serde`, `serde_json` | Serialization |
| Time | `time` 0.3 | Timestamps, durations |

### Frontend

| Area | Package |
|------|---------|
| Framework | `react` 18 |
| Build | `vite` 5 |
| Types | `typescript` 5 |
| Styling | `tailwindcss` 3 + `@tailwindcss/typography` |
| UI primitives | `shadcn/ui` (copy-pasted components, not a dep) |
| Icons | `lucide-react` |
| Server state | `@tanstack/react-query` 5 |
| Client state | `zustand` 4 |
| Router | `react-router-dom` 6 |
| Uploads | `react-dropzone` |
| Markdown | `react-markdown` + `remark-gfm` |
| Forms | `react-hook-form` + `zod` |
| HTTP | native `fetch` |
| Streaming | `EventSource` for GET streams; `fetch` + `ReadableStream` for POST streams (chat) |
| Package manager | `pnpm` |

### Infra

| Tool | Purpose |
|------|---------|
| Podman | Package and run Postgres + app (rootless, OCI-compatible) |
| podman compose | Dev and prod orchestration (also supports Quadlet/systemd) |
| Caddy | Reverse proxy + auto HTTPS (Let's Encrypt) |
| Postgres 16 + pgvector | Database + vector store |
| systemd / Quadlet | Podman-native alternative to compose for production |
| rsync / cron | Backups |

## Runtime model

- **Single tokio runtime.** Web handlers are async and do not block.
- **Blocking inference is offloaded.** `whisper-rs`, `llama-cpp-2`, and `fastembed` are CPU-bound and block. They run on `tokio::task::spawn_blocking` or a dedicated `rayon` thread pool. The HTTP layer streams tokens back via SSE using a bounded `mpsc` channel.
- **Background workers.** Ingestion is too slow to do in the request cycle. An upload creates a `documents` row with `status = 'pending'` and pushes a job onto an in-process queue. A worker pool (size `WORKER_THREADS`, default = `num_cpus::get() - 1`) drains the queue and updates row status through `transcribing → chunking → embedding → analysing → done`. Jobs are persisted in the `jobs` table so a crash doesn't lose work; a reaper resets stuck `running` jobs every minute (see [`02-data-model.md`](02-data-model.md#background-jobs)).
- **Concurrency knobs, separate from worker count.** A single `WORKER_THREADS` would otherwise allow all workers to simultaneously load a whisper context, which is RAM-heavy. Independently capped:
  - `MAX_CONCURRENT_WHISPER` (default 2): how many simultaneous transcription jobs. Governed by a `tokio::sync::Semaphore`. Other workers run text ingestion, embedding, analysis without blocking on the whisper semaphore.
  - `MAX_CONCURRENT_GENERATION` (default 2): how many LLM generation streams at once. LLM generation saturates all cores of a CPU box, so this is low.
  - `MAX_CONCURRENT_INGEST_PER_USER` (default 3): prevents one user filling the queue.
- **One binary.** No microservices. Axum, workers, and embedded model runtimes all compile into one `persona-ai` executable.

### RAM budget (4 vCPU / 16 GB VPS)

| Component | Count | RAM each | Subtotal |
|-----------|-------|----------|----------|
| LLM (Qwen2.5-7B Q4_K_M) | 1 | 5.5 GB | 5.5 GB |
| Whisper contexts (large-v3) | 2 | ~3.0 GB | 6.0 GB |
| Embedder (bge-m3 ONNX) | 1 (shared) | ~1.0 GB | 1.0 GB |
| Postgres (default tuning + pgvector) | 1 | ~1.5 GB | 1.5 GB |
| App overhead + misc | — | — | ~1.0 GB |
| OS + kernel | — | — | ~1.0 GB |
| **Total committed** | | | **~16.0 GB** |
| **Headroom** | | | ~0 GB on 16 GB; use 32 GB recommended |

On 16 GB boxes: set `MAX_CONCURRENT_WHISPER` to 1 (saves 3 GB), or swap to `whisper-medium` (~1.5 GB each). On 8 GB boxes: use `whisper-base.en` and a 3.8B LLM. The 32 GB tier is the comfortable production target for the multilingual stack.

## Directory layout on disk (VPS)

```
/opt/persona-ai/
  bin/persona-ai               # the compiled binary (cargo crate name: persona-ai)
  config/app.toml              # non-secret config
  .env                         # secrets (DB URL, Resend API key, session secret)

/data/
  uploads/<persona_id>/<doc_id>.<ext>    # original files
  transcripts/<doc_id>.txt               # whisper output
  exports/<user_id>/<timestamp>.docx     # user-triggered exports
  models/
    ggml-large-v3.bin
    bge-m3/
      model.onnx
      tokenizer.json
      config.json
    qwen2.5-7b-instruct-q4_k_m.gguf

/var/lib/postgresql/16/       # Postgres data dir (managed by distro or container volume)
```

## Cross-cutting concerns

### Authentication
Server-side sessions. Cookie is `HttpOnly; Secure; SameSite=Lax`. Session table lives in Postgres. Login calls `argon2::verify`, creates a session row, sets cookie. Logout deletes the row.

### Authorization
Every request that touches user data passes through a `require_auth` extractor that:
1. Loads the session.
2. Loads the user.
3. Injects a `UserCtx { user_id, role }` into the request.

Every repository function takes `user_id: UserId` as a parameter. There is no repository API that can bypass it. For pgvector queries: `WHERE user_id = $1 AND persona_id = $2 AND (era_id = $3 OR $3 IS NULL)`.

### Rate limiting
`tower-governor` applied to `/api/auth/*` and `/api/admin/invites`. 10 req/min/IP default.

### Error handling
A single `AppError` enum (thiserror) with `IntoResponse`. User-facing errors never leak internal detail; structured logs keep the context.

### Configuration
`app.toml` for non-secrets (model paths, chunk size, worker count). `.env` for secrets (DB URL, `RESEND_API_KEY`, `SESSION_SECRET`, `ADMIN_BOOTSTRAP_EMAIL`).

### Observability
`tracing` + `tracing-subscriber` with JSON output in prod, pretty in dev. Span for each request; worker jobs emit structured events. `/healthz` returns 200 if DB is reachable, migrations applied, and model files exist with matching SHA-256 (checksums listed in [`04-models.md`](04-models.md#file-integrity)). `/readyz` returns 200 once the LLM is loaded and at least one worker is idle.

### Error tracking
No external service (Sentry etc.). Instead, the error middleware writes every 5xx to an `errors` table (schema in [`02-data-model.md`](02-data-model.md#errors)) with route, code, message, backtrace, request_id, user_id. Admin UI `/admin/errors` lists recent entries. Rows retained 30 days.

## Related

- [`02-data-model.md`](02-data-model.md)
- [`03-design-system.md`](03-design-system.md)
- [`sprints/sprint-01-foundation.md`](sprints/sprint-01-foundation.md)
