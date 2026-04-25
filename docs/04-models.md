# Model selection — recommended models per task

All models run locally on CPU. Selections prioritise: (a) quality-per-RAM, (b) actively maintained, (c) permissive licence for private personal use, (d) good inference support in the chosen Rust runtimes.

**Language scope (v1): English only.** Whisper's `.en` variants, BGE's English model, and the default LLM are tuned for English. Ingesting non-English text will not crash but the Style Profile's English-specific analysers (function-word table, register classifier) will produce noisy output. Multilingual support is a v2 decision requiring: a multilingual embedding model, whisper-large-v3 (slower), and language-aware analysers.

## Quick summary

| Task | Recommended default | RAM at load | Format | Rust runtime |
|------|--------------------|-------------|--------|--------------|
| Speech-to-text | **`whisper-small.en`** (`ggml-small.en.bin`) | ~ 1 GB | GGML | `whisper-rs` |
| Text embeddings | **`BAAI/bge-small-en-v1.5`** | ~ 150 MB | ONNX | `fastembed` |
| Chat LLM | **`Qwen2.5-7B-Instruct` Q4_K_M** | ~ 5.5 GB | GGUF | `llama-cpp-2` |
| Reranker (optional) | **`BAAI/bge-reranker-base`** | ~ 250 MB | ONNX | `fastembed` |

Everything fits comfortably in a 16 GB VPS with room for Postgres and the OS.

## Speech-to-text (audio → text)

### Default: `whisper-small.en` (English-only)

- **File:** `ggml-small.en.bin` (~ 465 MB on disk, ~ 1 GB loaded).
- **Why:** sweet spot of accuracy and speed on CPU. Runs at ~ 0.5× realtime on a 4 vCPU box (1 hour audio → ~ 2 hours process). WER on clean audio is 5–7 %.
- **When to switch:** if users upload non-English audio.

### Alternatives

| Model | Size | When |
|-------|------|------|
| `whisper-base.en` | ~ 150 MB | Faster (~ 1× realtime), noticeably lower accuracy. Good for a dev box. |
| `whisper-medium.en` | ~ 1.5 GB | Better accuracy, roughly 0.3× realtime. Use if audio is noisy or accented. |
| `whisper-large-v3` | ~ 3 GB | Multilingual, best accuracy. Too slow on CPU for real-time UX; viable for overnight batch. |
| `whisper-tiny.en` | ~ 75 MB | For the lowest-spec VPS. Accuracy drops fast. Not recommended unless hardware demands it. |

**Licence:** MIT (OpenAI Whisper).
**Download:** `https://huggingface.co/ggerganov/whisper.cpp` — `*.bin` files.

### Preprocessing note

Whisper expects 16 kHz mono wav. Input files are transcoded via `ffmpeg` before transcription. We bundle `ffmpeg` in the Docker image.

## Text embeddings

### Default: `BAAI/bge-small-en-v1.5`

- **Dim:** 384.
- **Size:** ~ 130 MB ONNX.
- **Why:** consistently near-top of MTEB for its size. CPU-friendly. `fastembed` ships it out of the box.
- **Languages:** English optimised; handles basic multilingual but weakly.

### Alternatives

| Model | Dim | Size | When |
|-------|-----|------|------|
| `intfloat/e5-small-v2` | 384 | ~ 130 MB | Very close to BGE on benchmarks, sometimes better on asymmetric queries. |
| `BAAI/bge-base-en-v1.5` | 768 | ~ 440 MB | Meaningful quality bump; pgvector index grows 2×. Use if retrieval feels weak. |
| `nomic-ai/nomic-embed-text-v1.5` | 768 | ~ 550 MB | Great quality, task-specific prefixes. Permissive licence. |
| `sentence-transformers/all-MiniLM-L6-v2` | 384 | ~ 90 MB | Older but bulletproof; slightly weaker than BGE. |

**Dimension locks the schema.** Changing dim later means re-indexing all chunks and dropping/recreating the pgvector column + HNSW index. Pick once for v1.

**Licence:** Apache-2.0 / MIT across these options.

## Chat LLM (the generator)

### Default: `Qwen2.5-7B-Instruct` Q4_K_M GGUF

- **File:** `qwen2.5-7b-instruct-q4_k_m.gguf` (~ 4.5 GB disk, ~ 5.5 GB loaded).
- **Why:**
  - Top-tier quality for its size in early 2025; matches or beats Llama-3.1-8B on most writing benchmarks.
  - Excellent instruction-following. Holds a persona prompt under pressure.
  - Apache-2.0 licence.
  - First-class GGUF support; no conversion gymnastics.
- **Speed on CPU:** ~ 8–12 tok/s on 4 vCPU, ~ 12–18 tok/s on 8 vCPU. Acceptable streaming.
- **Chat template:** built into the GGUF; `llama.cpp` applies it via `llama_chat_apply_template`.

### Alternatives

| Model | Size (Q4_K_M) | When |
|-------|---------------|------|
| `Llama-3.1-8B-Instruct` | ~ 4.9 GB | Very close in quality. Meta licence (community) — fine for private use. Good fallback. |
| `Phi-3-mini-4k-Instruct` (3.8B) | ~ 2.3 GB | Much smaller; use on 8 GB RAM VPS. Noticeably weaker at holding voice. |
| `Mistral-7B-Instruct-v0.3` | ~ 4.4 GB | Strong open model; sometimes slightly more flowery prose than we want. |
| `Qwen2.5-14B-Instruct` Q4_K_M | ~ 9 GB | Best quality on CPU if you have ≥ 24 GB RAM. Speed drops to ~ 3–6 tok/s. Worth trying for the final product. |
| `Qwen2.5-3B-Instruct` | ~ 2 GB | Cheap, snappy; lower mimicry fidelity. Useful for dev and tests. |

### Quantisation choices

`llama.cpp` offers many quantisations; the useful ones for CPU:

| Quant | Relative size | Quality loss | Notes |
|-------|---------------|--------------|-------|
| Q4_K_M | 1.00× | Very low | Current sweet spot; our default. |
| Q5_K_M | 1.22× | Near-negligible | Use if you have the RAM. |
| Q6_K | 1.42× | Barely measurable | Near-FP16 quality; heavy. |
| Q3_K_M | 0.80× | Noticeable | Only for tight RAM. |
| Q8_0 | 1.75× | Effectively none | Use for reference benchmarks, not prod. |

Download Qwen2.5 GGUFs from `https://huggingface.co/Qwen/Qwen2.5-7B-Instruct-GGUF` or `https://huggingface.co/bartowski/Qwen2.5-7B-Instruct-GGUF`.

### Context window

- Default context: 8192 tokens. Plenty for system prompt (~ 1.5 k) + exemplars (~ 2 k) + retrieved chunks (~ 3 k) + conversation history.
- If persona prompts grow, raise to 16k or 32k; Qwen2.5 supports it but RAM climbs roughly linearly.

### Licences at a glance

| Model | Licence | Private-use OK |
|-------|---------|---------------|
| Qwen2.5 | Apache-2.0 | Yes |
| Llama-3.1 | Llama 3.1 Community | Yes |
| Mistral-7B | Apache-2.0 | Yes |
| Phi-3 | MIT | Yes |

## Reranker (optional, v1.5)

Retrieval quality often improves with a reranker applied to the top 30–50 candidates before truncating to 8.

### Default (if added): `BAAI/bge-reranker-base`

- ~ 250 MB ONNX.
- Adds ~ 50–150 ms per query on CPU for 30 candidates.
- Feature-flagged off in v1; revisit if relevance feels weak.

## Where model files live

```
/data/models/
  whisper/
    ggml-small.en.bin
  embeddings/
    bge-small-en-v1.5/
      model.onnx
      tokenizer.json
  llm/
    qwen2.5-7b-instruct-q4_k_m.gguf
  reranker/               # optional
    bge-reranker-base/
      model.onnx
      tokenizer.json
```

`fastembed` handles download/caching for embedding + reranker models if a path isn't preseeded. For prod we pre-download in the Docker build to avoid runtime fetches.

## File integrity

The backend verifies model files on startup. Expected SHA-256 hashes for the default set live in `backend/assets/models.toml`, shipped with the binary:

```toml
[whisper.small_en]
path = "whisper/ggml-small.en.bin"
sha256 = "1be3a9b2e3b77d5a0d8b3e6f...<pinned>"
size_bytes = 487601967

[embedding.bge_small_en_v1_5]
path = "embeddings/bge-small-en-v1.5/model.onnx"
sha256 = "...<pinned>"
size_bytes = 133093490

[llm.qwen2_5_7b_q4_k_m]
path = "llm/qwen2.5-7b-instruct-q4_k_m.gguf"
sha256 = "...<pinned>"
size_bytes = 4431391008
```

On startup:
1. For each configured model, open the file and compute SHA-256 (streamed, one pass).
2. If the file is absent → fail boot with a clear message: which model, expected path, download URL from the model table below.
3. If size mismatches → fail boot.
4. If hash mismatches → fail boot. This catches corrupted downloads and tampering.
5. `/healthz` fails with 503 until all checks pass.

The actual pinned hashes are recorded during first prod build (capture once, commit to repo). Updating a model is a deliberate, reviewed change: new hash in `models.toml`, new migration note, deploy.

### Download sources (defaults)

| Model | URL |
|-------|-----|
| whisper small.en | `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin` |
| bge-small-en-v1.5 | `https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main/onnx/model.onnx` |
| qwen2.5-7b-instruct-q4_k_m | `https://huggingface.co/bartowski/Qwen2.5-7B-Instruct-GGUF/resolve/main/Qwen2.5-7B-Instruct-Q4_K_M.gguf` |

A `scripts/download-models.sh` script fetches all three, verifies SHA-256, and places them correctly. Re-run to re-verify.

## How to change a model (runbook reference)

### Swap LLM

1. Place new GGUF in `/data/models/llm/`.
2. Set `LLM_MODEL_PATH=/data/models/llm/<new>.gguf` in `.env`.
3. Restart the backend. Sessions persist; old messages remain.

### Swap embedding model (breaking)

1. Create a new pgvector column or a new `chunks_v2` table with the new dim.
2. Re-embed every chunk in a background migration job.
3. Swap the column / table, rebuild the HNSW index, drop the old.
4. Set `EMBEDDING_MODEL=<new>` in config.

Document this as a rare, planned migration.

## Hardware sizing recap

| VPS spec | Feasible defaults |
|----------|-------------------|
| 2 vCPU / 8 GB | Phi-3 mini 3.8B, whisper-base.en, bge-small. Slow but works. |
| 4 vCPU / 16 GB | **Recommended defaults above.** |
| 8 vCPU / 32 GB | Qwen2.5-14B Q4_K_M, whisper-medium.en, bge-base. |
| 16 vCPU / 64 GB | Qwen2.5-14B Q6_K; still CPU — diminishing returns past 8 cores. |

GPU would change everything (15–30× generation speed, sub-realtime whisper, per-persona LoRA training), but is out of scope for v1.
