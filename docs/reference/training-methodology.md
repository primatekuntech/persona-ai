# Training methodology — how a persona is "trained"

This is the conceptual companion to sprint 4 (analysis) and sprint 5 (chat). It explains what "training a mimic persona" actually means in this system, why that approach works, where its limits are, and how to upgrade when GPU becomes available.

## The core claim

On a **CPU-only VPS**, we cannot practically update the weights of a 7B-parameter language model per persona. A single-epoch QLoRA on ~ 50 k tokens of text takes a good consumer GPU 30–90 minutes; on CPU it is orders of magnitude slower and not a viable v1 product flow.

Instead, we achieve **per-persona customisation** by separating what is expensive (weights) from what is cheap (context and retrieval). Every persona owns:

1. A **corpus** — their own writings and transcripts.
2. A **Style Profile** — a structured summary of that corpus (see `sprints/sprint-04-analysis.md`).
3. An **exemplar set** — canonical chunks that represent the corpus well.
4. A **persona prompt** — a system prompt assembled from the profile at chat time.
5. **Retrieval at inference** — for every query, the most relevant chunks from *this persona's* corpus are fetched and placed in context.

The LLM's weights do not change. What changes per persona is the *context* it operates in.

## Why this works for style mimicry

Modern instruction-tuned LLMs are strong stylistic mimics **when given good examples**. The instruction "write in the style of these excerpts" combined with actual excerpts plus a set of measurable stylistic constraints (sentence length, punctuation rhythm, vocabulary) reliably produces output that matches the target on:

- Word choice and register.
- Sentence rhythm and length distribution.
- Punctuation habits.
- Characteristic openings and sign-offs.
- Recurring imagery and metaphors (when present in exemplars).
- Topical gravity (what the persona tends to return to).

The profile gives the model measurable targets; the exemplars give it something to pattern-match against. Together they pin the output into a narrow stylistic band.

## Why this is not the same as fine-tuning

Prompt-based mimicry has real limits. It reproduces **surface style**, not deep mental models. A fine-tuned model can internalise:

- Reasoning tics (how this person justifies things, which premises they lean on).
- Long-range priors (consistent worldview across topics the training data never covered).
- Idiosyncratic failure modes (the specific ways this person gets confused or uses words wrongly).

Prompt-based mimicry approximates these through exemplars but cannot fully reproduce them. An experienced reader who knows the persona well may notice:

- Occasional slips into the base model's default voice, especially on novel topics.
- A tendency toward cleaner, more grammatical output than the persona's original.
- Missing the rarest vocabulary items if they don't appear in the retrieval set.

These limits are acceptable for the product we are building in v1. They are not acceptable forever.

## The pipeline in detail

### Offline (once per ingestion)

```
corpus (text + audio)
  │
  ├── text parsed / audio transcribed (whisper)
  │
  ├── chunked (~400 tokens, sentence-aware)
  │
  ├── embedded (bge-small-en-v1.5, 384 dim)   ──► pgvector HNSW index
  │
  └── analysed                                ──► style_profiles (JSONB)
        │
        ├── lexical (TTR, distinctive words, n-grams)
        ├── syntactic (sentence length, type mix, punctuation)
        ├── semantic (topics via clustering, entities, sentiment)
        ├── stylistic (openings, sign-offs, register)
        └── exemplars (5 canonical chunks)
```

### At query time

```
user query
  │
  ├── embedded
  │
  ├── hybrid retrieval (vector + trigram) filtered by user/persona/era
  │      │
  │      └── RRF fusion → top 8 chunks
  │
  ├── profile exemplars prepended (2–3 chunks)
  │
  └── prompt builder composes system prompt from:
          profile fields + exemplars + retrieved snippets
  │
  └── LLM generates (Qwen/Llama, streaming tokens via SSE)
```

Every piece in the prompt is either a measured fact about the persona or a verbatim sample of their writing. This is the training surface.

## Data requirements

Rough guidelines for "good enough":

| Corpus size | Quality of mimicry |
|-------------|--------------------|
| < 5 k words | Poor — profile is noisy, exemplars thin. |
| 5–20 k words | Adequate — surface style captured; may feel generic on novel topics. |
| 20–50 k words | Good — distinctive vocabulary and rhythm emerge. |
| 50–200 k words | Strong — profile stable, retrieval always finds something close. |
| > 200 k words | Diminishing returns for prompt-based mimicry; now worth fine-tuning. |

More is better up to ~ 200 k words, then the benefit tapers because the context window and retrieval set are bounded.

For audio: 1 hour of decent-quality speech → ~ 6–9 k transcribed words. Podcasts, interviews, voice notes are all usable.

## Eras and segmentation

Tagging documents with an era is the cleanest way to get different "versions" of a person. The system computes:

- One profile per era (filtered by `era_id`).
- One corpus-wide profile (all eras merged) as a fallback when an era has too little data.

At chat time, selecting an era filters retrieval and uses the era-specific profile. A 35-year-old user's "me at 15" persona can sit alongside "me at 25" with no cross-contamination.

When era metadata is missing, the system falls back to whole-corpus retrieval.

## Anti-anachronism

One non-obvious trick: the persona prompt includes explicit instructions **not to reference knowledge from after the era's end date**. Combined with retrieval filtered to that era, this markedly reduces anachronistic slips (e.g. a "15-year-old you" circa 2010 referencing TikTok).

This is imperfect — the base model still knows about TikTok. But the instruction plus the era-filtered exemplars push it away from post-era references most of the time.

## Upgrade path — when GPU becomes available

If the user ever has access to a GPU (cloud spot instance, local RTX, Apple Silicon MLX), the upgrade is clean:

1. Keep the current pipeline as baseline.
2. Add a `POST /api/personas/:id/lora/train` job that:
   - Exports persona chunks to a training JSONL.
   - Runs QLoRA for 1–3 epochs on top of the base model (Qwen2.5-7B).
   - Saves the adapter to `/data/loras/<persona_id>.safetensors`.
3. At chat time, load the persona's LoRA adapter on top of the base model (`llama.cpp` supports runtime LoRA merging).
4. Keep the retrieval and the prompt profile — they still help, and now compound with fine-tuning.

Training recipe (to be documented in a later sprint):
- Format: instruction-free self-modelling. Pairs like `{input: "", output: chunk}` or sentence-level next-prediction on the persona's text.
- Batch size 1, gradient accumulation 16, rank 16, alpha 32, dropout 0.05.
- Learning rate 2e-4 with cosine schedule.
- Stop when validation perplexity plateaus (typically 1–3 epochs on a 50 k token corpus).
- Guardrail: always keep a RAG-only fallback available so the user can A/B the fine-tuned version against the baseline.

## What we are not claiming

- We are not digitising a person's mind.
- We are not reconstructing memories; anything the model says about specific events is either (a) grounded in a retrieved chunk or (b) confabulation styled to sound like the persona.
- A persona can change how they *sound* in the output, but not what they *know*. The base LLM's knowledge remains the knowledge ceiling.
- This is an assistive creative tool, not a truth oracle about the target person.

## Validation

Two checks we should run after every significant profile change:

1. **Blind comparison.** Take 3 real excerpts from the persona's corpus and 3 generated samples. Ask a reader who knows the persona to guess which is which. > 60 % recognition of real-vs-generated means mimicry is working.
2. **Metric delta.** Compute the Style Profile of a set of generated outputs and compare to the persona's ground-truth profile. Sentence length, punctuation rhythm, contractions rate should all fall within ± 20 % of ground truth.

These validations are not part of the v1 shipping product, but they are recommended during development to tune prompt templates and retrieval counts.
