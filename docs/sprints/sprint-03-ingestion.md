# Sprint 3 — Ingestion: upload, transcribe, chunk, embed

**Goal:** users upload text and audio files to a persona. The system parses/transcribes, chunks, and embeds them. Progress is visible in the UI. The corpus is searchable by vector similarity.

**Duration estimate:** 6–8 days. This sprint is the heaviest.

## Deliverables

1. Upload endpoint (multipart) with per-persona / per-era targeting, idempotency keys, quota enforcement, and duplicate detection.
2. Text extraction for `.txt`, `.md`, `.docx`, `.pdf` — with PDF/docx parser sandboxing (page limits, memory caps).
3. Audio transcription for `.mp3`, `.wav`, `.m4a` via whisper.cpp with duration and sample-rate limits.
4. Sentence-aware chunker using the **bge tokenizer** (not tiktoken) for accurate chunk sizes.
5. CPU embedding via `fastembed` (bge-small-en-v1.5); chunks land in DB with `embedding IS NULL` first, populated during the embed phase.
6. pgvector insert + HNSW index usage confirmed via EXPLAIN.
7. Background job queue (persisted in `jobs` table) with a **stuck-job reaper** that re-queues `running` jobs whose `heartbeat_at` has not ticked in 2 minutes (workers heartbeat every 30 s).
8. Per-user storage and document-count quotas enforced atomically at upload.
9. Frontend: upload dropzone, document list, live status via SSE.

## Dependencies this sprint adds

### Backend

- `whisper-rs` + models downloaded to `/data/models/` (see [`../04-models.md`](../04-models.md)).
- `fastembed` crate.
- `text-splitter` crate.
- `docx-rs` (for reading), `lopdf`, `pulldown-cmark`, `encoding_rs`.
- `infer` for MIME sniffing.
- `tokio` tasks + a dedicated blocking thread pool for ML.

## Backend tasks

### 3.1 Upload endpoint

```
POST /api/personas/:id/documents
  headers:
    Idempotency-Key: <uuid>   (required)
  multipart fields:
    file:        required
    era_id:      optional UUID
    title:       optional
    source:      optional
```

Flow:
1. `require_auth`, verify persona ownership (404 if not caller's).
2. Check `Idempotency-Key` per [`../06-api-conventions.md`](../06-api-conventions.md#idempotency) — if a record exists for `(user_id, key)` with the same request hash, return the stored response.
3. Stream the file to a temp path under `/data/uploads/.tmp/<ulid>` while accumulating a streaming SHA-256 and a byte count. Abort early if byte count exceeds the kind-specific limit (don't wait until full-body disk write finishes).
4. Sniff MIME with `infer` on the first 512 bytes of the temp file. Validate against the allow-list below. Reject else 415.
5. Determine `kind`: `text` or `audio`.
6. **Duplicate check.** Compute `content_hash = hex(sha256)`. `SELECT id FROM documents WHERE persona_id = $1 AND content_hash = $2` — if found, delete the temp file and return **409** `{ "error": { "code": "duplicate", "message": "This document is already uploaded." }, "document_id": "<existing>" }`. The unique index on `(persona_id, content_hash)` makes the check race-safe on insert.
7. **Quota check (atomic).**
   ```sql
   UPDATE users
   SET current_storage_bytes = current_storage_bytes + $size,
       current_doc_count     = current_doc_count + 1
   WHERE id = $uid
     AND current_storage_bytes + $size <= quota_storage_bytes
     AND current_doc_count + 1            <= quota_doc_count
   RETURNING current_storage_bytes;
   ```
   Zero rows returned → 413 `{ "error": { "code": "quota_exceeded", "message": "Storage or document limit reached." } }`; delete temp file.
8. Move to final path `/data/uploads/<persona_id>/<document_id>.<ext>`.
9. Insert `documents` row: `status='pending'`, `content_hash`, `size_bytes`, `progress_pct=NULL`.
10. Enqueue `jobs(kind='ingest_document', payload={document_id}, scheduled_at=now())`.
11. Respond 201 with the document row.

If any step after (7) fails, the quota counters are decremented in the same transaction that handles the failure.

**Limits (enforced both as multipart streaming limits and as body caps in axum):**

| Kind | MIME allow-list | Max file | Other caps |
|------|-----------------|----------|-----------|
| text | `text/plain`, `text/markdown`, `application/pdf`, `application/vnd.openxmlformats-officedocument.wordprocessingml.document` | 25 MB | PDF ≤ 500 pages, docx ≤ 5000 paragraphs |
| audio | `audio/mpeg`, `audio/wav`, `audio/x-wav`, `audio/mp4`, `audio/x-m4a` | 500 MB | duration ≤ 6 h (probed via `ffprobe`) |

Per-request timeout: 600 s. Uploads exceeding it return 408.

### 3.1.1 Parser sandboxing

PDF and docx parsers are common attack surfaces (zip-bombs, infinite loops). Defences:

- PDF via `lopdf`: reject `/Encrypt` dictionaries (encrypted PDFs), reject if advertised page count > 500, enforce `RLIMIT_AS = 512 MB` at the **ingest worker process** level (set once in `main.rs` via `libc::setrlimit(RLIMIT_AS, ...)` on Linux; a no-op on non-Linux dev boxes, documented in the runbook). This is a process-wide virtual-memory cap shared across all ingest jobs, not a per-thread or per-SQL limit — `RLIMIT_AS` does not exist at thread granularity on Linux. A runaway parser crashes the ingest process with ENOMEM; the reaper then re-queues the job. If we later need per-job isolation we'd fork a helper process per parse.
- docx via `docx-rs`: cap `paragraphs` iteration at 5000, reject total extracted text > 10 MB.
- Zip-bomb detection: both `.docx` (really a zip) and any file whose parser unzips must check the **uncompressed / compressed** ratio; reject anything above 100:1 at a size > 50 MB uncompressed.
- Parsers run inside `spawn_blocking` so a runaway parse does not stall the tokio runtime. If a parse exceeds 60 s CPU, the job fails with `error = 'parse_timeout'`.

### 3.2 Background worker

`services/worker.rs` runs at startup, polls `jobs` with `SELECT ... FOR UPDATE SKIP LOCKED LIMIT 1 WHERE status='queued' AND scheduled_at <= now()` to pick work. Workers count = `worker_threads` from config. Each job iteration:

1. Mark job `running` (set `started_at`, clear `scheduled_at`).
2. Dispatch by `kind`. Whisper dispatches acquire a `MAX_CONCURRENT_WHISPER` semaphore; text parses do not.
3. On success: mark `done`, commit.
4. On error: increment `attempts`; if `< 3`, requeue with `scheduled_at = now() + 2^attempts seconds` (exponential backoff); if `≥ 3`, mark `failed` and set `documents.status = 'failed'` with user-safe `error` text. The counters in `users` are **not** decremented — the bytes are still on disk until the user hits reingest or delete.

#### 3.2.1 Stuck-job reaper

A dedicated tokio task runs every 60 s:

```sql
UPDATE jobs
SET status = 'queued',
    scheduled_at = now() + interval '30 seconds',
    worker_id = NULL,
    heartbeat_at = NULL,
    attempts = attempts + 1,
    last_error = 'reaped: heartbeat expired'
WHERE status = 'running'
  AND heartbeat_at < now() - interval '2 minutes';
```

Catches jobs orphaned by a crashed worker or a power-cycle. Workers heartbeat by updating `jobs.heartbeat_at = now()` every 30 s during long-running transcriptions, so a live worker's job never ages past the reaper threshold.

The 2-minute window corresponds to four missed 30-second heartbeats — comfortably past any normal GC / swap pause, tight enough that a crashed worker's job recovers inside a user's patience budget. `started_at` is preserved for audit; only `heartbeat_at` and `worker_id` are reset.

#### 3.2.2 Per-user ingest concurrency cap

Inside the worker dispatcher: before starting an `ingest_document` job, check `SELECT count(*) FROM jobs WHERE kind='ingest_document' AND status='running' AND user_id = $1`. If `>= MAX_CONCURRENT_INGEST_PER_USER` (default 3), leave the job queued with `scheduled_at = now() + interval '5 seconds'`. Uses the indexed `jobs(user_id, status)` column from [`../02-data-model.md`](../02-data-model.md#background-jobs), not a JSONB probe. Prevents one user monopolising the worker pool; fair but simple. Formal priority queues are out of scope for v1.

### 3.3 ingest_document job

Steps (update `documents.status` at each transition; each transition is its own committed transaction so a crash resumes cleanly):

1. **Parse / transcribe.** `status = 'transcribing'` for audio; `status = 'parsing'` for text.
   - Text: read file → detect encoding (`encoding_rs`) → parse to plain text based on mime, applying §3.1.1 sandbox limits.
   - Audio: call whisper (see 3.4). Save transcript to `/data/transcripts/<doc_id>.txt`. Set `transcript_path`.
2. **Chunk.** `status = 'chunking'`. See 3.5. Insert `chunks` rows with `embedding = NULL` (the column is nullable per [`../02-data-model.md`](../02-data-model.md)) and `ordinal` set. Commits in batches of 500.
3. **Embed.** `status = 'embedding'`. See 3.6. Select chunks `WHERE document_id = $1 AND embedding IS NULL` in batches of 16, compute, `UPDATE chunks SET embedding = $2 WHERE id = $3`. Crash mid-embed → worker reaper re-runs, the "skip already-embedded chunks" filter in the SELECT makes it idempotent.
4. **Analyse.** Enqueue a `recompute_profile` job for the persona (handled in sprint 4). The document moves to `done` now; profile recompute runs async and does not block document completion.
5. Set `ingested_at = now()`, `status = 'done'`.

If any step fails terminally (§3.2 step 4), `documents.status = 'failed'` and chunks written so far are **kept** — they are useful for debugging and will be truncated on reingest.

### 3.4 Whisper integration

- Use `whisper-rs` with `ggml-small.en.bin` default (see [`../04-models.md`](../04-models.md)).
- All calls inside `tokio::task::spawn_blocking`, behind the `MAX_CONCURRENT_WHISPER` semaphore.
- Convert inputs to 16 kHz mono wav by shelling out to `ffmpeg` (bundled in the Docker image). Simpler and covers every codec we accept. `ffprobe` first to read duration; reject `> 6 h` with `audio_too_long`.
- Chunk long audio into 30 s windows with 1 s overlap; run whisper per window; concatenate text. whisper.cpp does this internally, but explicit windowing lets us update `progress_pct` after each window.
- `progress_pct` on `documents` is populated only during transcription and returned in the SSE events (§3.9).

`progress_pct` is already in the init migration (see [`../02-data-model.md`](../02-data-model.md)), so no ALTER is needed this sprint.

### 3.5 Chunking

`services/chunker.rs`:

- Target 400 tokens per chunk, 50 token overlap.
- **Use the bge tokenizer**, not tiktoken. Load `tokenizer.json` shipped alongside `bge-small-en-v1.5` via the `tokenizers` crate; wire it into `text-splitter::TextSplitter::new(HuggingFaceTokenizer::new(...))`. Using a mismatched tokenizer (e.g. tiktoken) silently over- or under-counts by 10–20 % and breaks the retrieval window budget in sprint 5.
- Preserve sentence boundaries: the splitter falls back on `.`, `!`, `?`, `\n\n`, `\n` in that order.
- Normalise whitespace (collapse runs of spaces, strip leading/trailing). Unicode-normalise to NFC.
- Discard chunks with `< 20` tokens (boilerplate, page numbers, empty lines).
- Record `token_count` on each chunk row (it's cheap to compute once and saves recomputation for the retrieval budget later).

### 3.6 Embedding

`services/embedder.rs`:

```rust
pub struct Embedder { model: TextEmbedding }

impl Embedder {
    pub fn new(model_dir: &Path) -> Result<Self>;
    pub fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;  // 384-dim
}
```

- `fastembed::TextEmbedding` loaded from local ONNX, model `BAAI/bge-small-en-v1.5`.
- Batch size 16 (fits comfortably in < 300 MB RAM).
- L2-normalise output so `vector_cosine_ops` behaves identically to dot product.
- Inside `spawn_blocking`.

### 3.7 pgvector insertion

Batch insert via `INSERT INTO chunks (...) VALUES ($1, $2, ..., $N)` with `pgvector::sqlx::Vector` wrapper. Commit in transactions of up to 500 chunks.

Sanity query (dev only):

```sql
SELECT text, embedding <=> $1 AS distance
FROM chunks
WHERE user_id = $2 AND persona_id = $3
ORDER BY embedding <=> $1
LIMIT 10;
```

### 3.8 Endpoints

```
GET    /api/personas/:id/documents               list (cursor pagination)
GET    /api/personas/:id/documents/:doc_id       one, with status
DELETE /api/personas/:id/documents/:doc_id       removes row + file + chunks + transcript
POST   /api/personas/:id/documents/:doc_id/reingest   re-runs pipeline (idempotency-key required)
GET    /api/personas/:id/documents/:doc_id/transcript text/plain
```

DELETE flow: `DELETE FROM documents WHERE id = $1 AND user_id = $2` cascades to `chunks` and `jobs` via FKs; then `UPDATE users SET current_storage_bytes = current_storage_bytes - $size, current_doc_count = current_doc_count - 1 WHERE id = $uid` in the same transaction; then async delete of `/data/uploads/<persona>/<doc>.<ext>` and `/data/transcripts/<doc>.txt`.

### 3.9 SSE status stream

```
GET /api/personas/:id/documents/events      SSE (authenticated via cookie)
  Emits: { document_id, status, progress_pct, error? } as the worker transitions state.
```

Implemented with a broadcast channel: workers emit events keyed by `user_id`; the SSE handler subscribes and filters by the persona id in the URL. Heartbeat comment line every 15 s to keep the connection alive through Caddy's idle timeout. Client reconnects with `Last-Event-Id` via the standard EventSource semantics (this is a pure GET read stream, so `EventSource` is fine here — only the chat generation stream in sprint 5 needs POST+fetch, because it carries a request body).

## Frontend tasks

### 3.10 Upload page

`/personas/:id/upload`:
- Dropzone covering the main area. Accepts multiple files.
- Optional era selector above dropzone.
- Optional "source" text input applied to all files in this batch.
- On drop → one `POST /api/personas/:id/documents` per file, concurrent up to 3.
- Each upload appears as a row with a progress bar for the HTTP upload, then transitions to the ingestion progress from the SSE stream (or 3 s polling if SSE not built yet).

### 3.11 Documents list

`/personas/:id/documents`:
- Table: title, kind, era, word_count or duration, status (dot + label), uploaded at, menu (re-ingest, delete, view transcript).
- Status colours: pending (zinc), transcribing/chunking/embedding/analysing (amber), done (green), failed (red).
- Filter by era, kind, status.

### 3.12 Transcript viewer

Modal / side sheet shows the plain transcript. Read-only in v1 (editable in a later sprint).

## Performance notes

- Target throughput on a 4 vCPU VPS:
  - Text ingestion: ~ 100 k tokens / minute end-to-end.
  - Audio transcription: `small.en` → ~ 0.5× realtime (1 h audio → 2 h process). `base.en` → ~ 1× realtime.
- Keep only one whisper context in memory per worker thread. Loading is ~ 1 s.
- Keep one `TextEmbedding` instance process-wide; it's thread-safe.
- `pg_stat_activity` occasionally; if idle connections pile up, lower `SQLX_MAX_CONNECTIONS`.

## Acceptance tests

1. Upload a 10 kB `.txt` → status reaches `done` in < 5 s; chunks rows exist with non-null embeddings; EXPLAIN on the retrieval query (sprint 5 preview) shows `Index Scan using chunks_embedding_hnsw`.
2. Upload a 30-minute `.mp3` → transcript file appears; chunks created; `done` within ~ 60 min on a 4 vCPU dev box. `progress_pct` increases monotonically during transcribing, is reset to NULL afterwards.
3. Upload unsupported `.zip` → 415.
4. Upload a file whose `Content-Type` header says `audio/mpeg` but whose magic bytes are GIF → 415 (infer wins).
5. Upload 3 files concurrently → all finish; no job left in `running` after completion.
6. Same user submits 10 simultaneous uploads → at most 3 run at once; rest sit in `queued`.
7. Kill backend mid-ingestion; restart → unfinished jobs resume; no duplicate chunks (embed phase is idempotent because it filters `embedding IS NULL`).
8. Force a worker to hang (SIGSTOP for 3 min, past the 2-minute heartbeat window) → reaper resets the job to `queued`, another worker picks it up.
9. Upload the same 1 MB file to the same persona twice → first returns 201, second returns 409 `duplicate` and does not consume additional storage.
10. Upload enough documents to hit `quota_doc_count` → next upload returns 413 `quota_exceeded` and the temp file is gone.
11. Upload a 1 GB `.mp3` → 413 before any streaming to disk (axum body cap stops it early).
12. Upload a maliciously crafted PDF with 100k pages in its `/Pages` tree → rejected with `parse_failed` and parser memory stays under RLIMIT.
13. Submit a second POST with the same `Idempotency-Key` and body hash → same 201 response, only one document row created.
14. Delete a document → file, transcript, chunks, embeddings gone; `users.current_storage_bytes` decremented by exactly the file size; quota headroom restored.

## Out of scope

- Style profile (sprint 4).
- Chat / retrieval (sprint 5).
- Editing transcripts.
- Incremental re-embedding when a model version changes (documented but not automated).
