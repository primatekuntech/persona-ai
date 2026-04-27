# Model selection — recommended models per task

All models run locally on CPU by default. Users can opt in to cloud providers (OpenAI-compatible endpoints, Google Speech) via Settings → Integrations. Selections prioritise: (a) quality-per-RAM, (b) actively maintained, (c) permissive licence for private personal use, (d) good inference support in the chosen Rust runtimes.

**Language scope:** Bahasa Malaysia, English, Mandarin (Simplified and Traditional), Cantonese, Tamil, and any language Whisper large-v3 supports. The embedding model (bge-m3) covers 100+ languages. Language is auto-detected per document and used to route audio to the best available transcription provider.

## Quick summary

| Task | Recommended default | RAM at load | Format | Rust runtime |
|------|--------------------|-------------|--------|--------------|
| Speech-to-text | **`whisper-large-v3`** (`ggml-large-v3.bin`) | ~ 3 GB | GGML | `whisper-rs` |
| Text embeddings | **`BAAI/bge-m3`** | ~ 1 GB | ONNX | `fastembed` |
| Language detection | **`lingua`** (Rust crate, built-in) | < 50 MB | — | `lingua` |
| Chat LLM | **`Qwen2.5-7B-Instruct` Q4_K_M** | ~ 5.5 GB | GGUF | `llama-cpp-2` |
| Reranker (optional) | **`BAAI/bge-reranker-base`** | ~ 250 MB | ONNX | `fastembed` |

The full multilingual stack is tight on 16 GB. Comfortable target is 32 GB. See hardware sizing below.

## Speech-to-text (audio → text)

### Default: `whisper-large-v3` (multilingual)

- **File:** `ggml-large-v3.bin` (~ 1.5 GB on disk, ~ 3 GB loaded).
- **Why:** best-in-class accuracy across all supported languages including Malay, Mandarin (`zh`), Cantonese (`yue`), and Tamil. Supports 99 languages. WER on clean English ~3 %, Mandarin ~6 %.
- **Language routing:** the worker passes a `language` hint derived from `documents.detected_language`. Cantonese uses `yue` rather than `zh` to prevent Whisper from producing Mandarin output.
- **Cloud fallback:** users who add a Google Speech provider get better `yue-Hant-HK` support; the provider registry tries it first for Cantonese audio.

### Alternatives

| Model | Size | When |
|-------|------|------|
| `whisper-medium` | ~ 1.5 GB (1.5 GB loaded) | Decent multilingual accuracy; half the RAM cost. Good 16 GB compromise. |
| `whisper-base.en` | ~ 150 MB | English-only dev/test. Not for production. |
| `whisper-small.en` | ~ 465 MB | English only; faster than large-v3 but drops multilingual support. |

**Licence:** MIT (OpenAI Whisper).
**Download:** `https://huggingface.co/ggerganov/whisper.cpp` — `ggml-large-v3.bin`.

### Preprocessing note

Whisper expects 16 kHz mono wav. Input files are transcoded via `ffmpeg` before transcription. We bundle `ffmpeg` in the Docker image.

## Text embeddings

### Default: `BAAI/bge-m3`

- **Dim:** 1024.
- **Size:** ~ 570 MB ONNX (~ 1 GB loaded).
- **Why:** multilingual (100+ languages), state-of-the-art retrieval across all Malaysian languages. Supports dense, sparse, and multi-vector retrieval modes; we use dense only.
- **Languages:** Malay, English, Simplified Chinese, Traditional Chinese, Cantonese (via Chinese tokenisation), Tamil, and 95+ more.

### Alternatives

| Model | Dim | Size | When |
|-------|-----|------|------|
| `BAAI/bge-base-en-v1.5` | 768 | ~ 440 MB | English-only but strong; use if corpus is pure English and RAM is tight. |
| `intfloat/multilingual-e5-base` | 768 | ~ 550 MB | Alternative multilingual model; slightly weaker than bge-m3. |
| `BAAI/bge-small-en-v1.5` | 384 | ~ 130 MB | English dev/test only. Not production-suitable for Malaysian content. |

**Dimension locks the schema.** Changing dim later means re-embedding all chunks and rebuilding the HNSW index. Existing chunks generated with a different dim must be re-embedded before they become retrievable.

**Licence:** Apache-2.0 / MIT across these options.

## Language detection

### `lingua` (built-in Rust crate)

Language detection runs in-process with no separate model file. The `lingua` crate is compiled with support for English, Malay, Chinese, and Tamil (plus Cantonese via a character-frequency heuristic layered on top — most CJK models conflate Cantonese with Mandarin).

Detection runs in `spawn_blocking` after text extraction. The result is stored in `documents.detected_language` as a BCP-47 code:

| Language | Stored code |
|----------|-------------|
| English | `en` |
| Bahasa Malaysia | `ms` |
| Mandarin (Simplified) | `zh-Hans` |
| Mandarin (Traditional) | `zh-Hant` |
| Cantonese | `yue` |
| Tamil | `ta` |
| Other | raw BCP-47 code (e.g. `iba` for Iban) |

For audio documents, language is detected by Whisper and stored back to `detected_language` after transcription.

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
    ggml-large-v3.bin
  embeddings/
    bge-m3/
      model.onnx
      tokenizer.json
  llm/
    qwen2.5-7b-instruct-q4_k_m.gguf
  reranker/               # optional
    bge-reranker-base/
      model.onnx
      tokenizer.json
```

`fastembed` handles download/caching for embedding + reranker models if a path isn't preseeded. For prod we pre-download in the Podman build to avoid runtime fetches.

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
| whisper large-v3 | `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin` |
| bge-m3 | downloaded automatically by `fastembed` on first run, or pre-seed via `scripts/download-models.sh` |
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
| 2 vCPU / 8 GB | Phi-3 mini 3.8B, whisper-base.en, bge-small-en. English only. Slow. |
| 4 vCPU / 16 GB | Qwen2.5-7B, whisper-medium, bge-m3. `MAX_CONCURRENT_WHISPER=1`. Multilingual. |
| **8 vCPU / 32 GB** | **Recommended for multilingual stack.** whisper-large-v3, bge-m3, Qwen2.5-7B with headroom. |
| 16 vCPU / 64 GB | Qwen2.5-14B Q4_K_M, whisper-large-v3, bge-m3. |

GPU would change everything (15–30× generation speed, sub-realtime whisper, per-persona LoRA training), but is out of scope for v1.
