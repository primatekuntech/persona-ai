# Sprint 4 — Analysis: per-persona style profile

**Goal:** after ingestion, the system extracts a rich **Style Profile** from each persona's corpus. The profile is the offline "training" artefact that drives mimicry in chat. It's also the content of the persona dashboard.

**Duration estimate:** 4–5 days.

## Deliverables

1. `style_profiles` table populated.
2. `recompute_profile` background job.
3. Analysis pipeline that computes lexical, syntactic, semantic, and stylistic metrics.
4. Per-era profiles (when eras exist) plus a persona-wide profile.
5. Frontend: persona dashboard renders the profile.

## What is a "Style Profile"?

A JSONB document stored in `style_profiles.profile`. It is both (a) human-readable on the dashboard and (b) the source of facts injected into the persona prompt at chat time.

### Profile JSON schema (v1)

```json
{
  "version": 1,
  "corpus": {
    "document_count": 42,
    "chunk_count": 890,
    "word_count": 185000,
    "date_range": ["2010-01-01", "2012-12-31"]
  },
  "lexical": {
    "type_token_ratio": 0.178,
    "avg_word_length": 4.7,
    "vocabulary_level": "intermediate",
    "distinctive_words": [
      {"word": "perhaps", "score": 7.2},
      {"word": "somehow", "score": 5.1}
    ],
    "characteristic_bigrams": [
      {"phrase": "i think", "count": 412},
      {"phrase": "for some reason", "count": 88}
    ],
    "characteristic_trigrams": [
      {"phrase": "i don't know", "count": 54}
    ],
    "function_word_profile": {
      "i": 0.038, "the": 0.041, "and": 0.033, "but": 0.018, "so": 0.014
    }
  },
  "syntactic": {
    "avg_sentence_length": 16.2,
    "sentence_length_distribution": {
      "short (<10)": 0.31, "medium (10-20)": 0.49, "long (>20)": 0.20
    },
    "sentence_type_mix": {
      "declarative": 0.78, "interrogative": 0.09, "exclamatory": 0.04, "fragment": 0.09
    },
    "punctuation_rhythm": {
      "comma_per_sentence": 1.4,
      "em_dash_per_1000_words": 3.1,
      "ellipsis_per_1000_words": 1.2,
      "semicolon_per_1000_words": 0.3
    },
    "paragraph_length_avg_sentences": 4.1
  },
  "semantic": {
    "top_topics": [
      {"label": "school and friends", "weight": 0.22, "keywords": ["class", "friend", "teacher"]},
      {"label": "self-doubt",         "weight": 0.14, "keywords": ["afraid", "wonder", "why"]}
    ],
    "recurring_entities": [
      {"entity": "Maya", "count": 63, "kind": "person"},
      {"entity": "Penang", "count": 22, "kind": "place"}
    ],
    "sentiment_baseline": {"polarity": -0.05, "subjectivity": 0.71}
  },
  "stylistic": {
    "opening_gambits": [
      "i remember when",
      "maybe i'm wrong but",
      "today was"
    ],
    "sign_offs": [
      "anyway",
      "that's all for now"
    ],
    "recurring_metaphors": [
      "life is a tide",
      "heart as a drum"
    ],
    "register": "casual-reflective",
    "first_person_rate": 0.041,
    "contractions_rate": 0.63
  },
  "exemplars": [
    {"chunk_id": "…", "score": 0.93, "reason": "high TTR, typical rhythm"},
    {"chunk_id": "…", "score": 0.91, "reason": "canonical opening"}
  ]
}
```

### Why these fields?

They map to what an LLM needs in its system prompt to imitate a style:

- **Distinctive words / bigrams** → the vocabulary it should reach for.
- **Sentence length + rhythm** → how long to make sentences.
- **Sentence type mix** → when to use questions, fragments.
- **Punctuation rhythm** → the persona's "music".
- **Opening gambits / sign-offs** → recognisable structural tics.
- **Top topics** → what this persona tends to talk about, even when prompted freely.
- **Exemplars** → few-shot anchors retrieved every time (see sprint 5).

## Backend tasks

### 4.1 `recompute_profile` job

Payload: `{ persona_id, era_id? }`. Enqueued:
- After each document's ingestion completes (coalesced: if one already queued for the same (persona, era) key, skip).
- On-demand via `POST /api/personas/:id/profile/recompute`.

Coalescing: the unique partial index is already in the init migration (see [`../02-data-model.md`](../02-data-model.md)):
```sql
CREATE UNIQUE INDEX jobs_recompute_profile_uniq
  ON jobs ((payload->>'persona_id'), (payload->>'era_id'))
  WHERE kind = 'recompute_profile' AND status = 'queued';
```
Enqueue uses `INSERT ... ON CONFLICT DO NOTHING` against this index so duplicate enqueues are no-ops.

### 4.1.1 Corpus-size quality floor

Style metrics are statistically noisy on small corpora. Before running the pipeline:

```
corpus_tokens = SUM(chunks.token_count) for the scope
if corpus_tokens < 2000:
    write style_profiles row with profile = {
        "version": 1,
        "status": "insufficient_corpus",
        "corpus": { ...actual counts... },
        "message": "Upload at least ~2000 words (roughly 4 pages) to generate a reliable style profile."
    }
    return  // skip the heavy analyses
```

2000 tokens ≈ 1500 words ≈ one short essay. Below that, TTR is meaningless (vocabulary hasn't saturated), topic clustering collapses, and the profile looks misleadingly precise. The dashboard renders this friendly state instead of a bad profile. The ingestion flow still enqueues recomputes on every doc — once the corpus crosses the floor, the next recompute produces a real profile automatically.

### 4.1.2 Incremental recompute (v2 note)

v1 recomputes the whole profile from scratch every time. For a 200 k-word corpus this is ~5 s — acceptable. If corpora grow to millions of words, an incremental approach (running n-gram counts, streaming sentence stats, k-means warm-start with previous centroids) is a v2 project. Not in scope; documented so we remember not to add partial incrementality now.

### 4.2 Pipeline

Pure functions, one per metric family. Operate on the set of chunks for the persona (and era if given).

```
fn build_profile(chunks: &[Chunk], ctx: ProfileCtx) -> StyleProfile
```

Compose:
- `lexical::compute(chunks)` →
  - tokenise (lowercase, strip punctuation for TTR; keep casing for bigram extraction).
  - TTR, avg word length.
  - distinctive words via TF-IDF against a baseline corpus. Baseline = a frozen in-repo JSON of English word frequencies (e.g. from `wordfreq` pre-computed; ship as `assets/english_word_freq.json`).
  - n-gram counting (1–3 grams) with min-count thresholds.
- `syntactic::compute(chunks)` →
  - sentence split on `[.!?]\s+` with small tolerance for quotes.
  - length stats, type classification (regex-based), punctuation counts.
- `semantic::compute(chunks)` →
  - topic discovery: embed chunk *summaries* with the existing embedder, then k-means (k = 6–10) via `linfa-clustering`. Label each cluster with its 5 top TF-IDF words.
  - entities: `rust-bert`? too heavy. Instead, v1 uses a light heuristic — capitalised bigrams + frequency — and mark as unknown kind. Can swap for NER later.
  - sentiment: `vader_sentiment` port or hand-rolled lexicon (ship a short file). Polarity average across chunks.
- `stylistic::compute(chunks, lexical, syntactic)` →
  - opening_gambits: most common first 3–5 tokens across chunks/paragraphs (normalised).
  - sign_offs: most common last 3–5 tokens of paragraphs, excluding periods.
  - recurring_metaphors: v1 skip (hard); leave array empty with a comment. v2 could use LLM-in-the-loop to extract.
  - register classification: small rule-based decision tree using contractions rate, first-person rate, avg sentence length, interrogative rate.
- `exemplars::pick(chunks, profile)` →
  - Pick 5 chunks whose metrics are closest to the corpus median — i.e. the most "canonical" examples. Score = inverse distance to centroid in a small feature space (sentence length, TTR, punctuation rhythm).
  - Store `chunk_id` + `reason` only; do not duplicate text.

All heavy math in `rayon` parallel iterators. Target < 5 s for a 200 k word corpus.

### 4.3 Storage

UPSERT into `style_profiles` by `(persona_id, era_id)` unique key.

Also compute era-specific profiles for every era that has ≥ 1 chunk. The persona-wide profile uses all chunks (era = NULL).

### 4.4 Endpoints

```
GET  /api/personas/:id/profile                returns persona-wide profile
GET  /api/personas/:id/eras/:era_id/profile   era-specific profile
POST /api/personas/:id/profile/recompute      enqueues job (idempotency-key required)
```

Response shape always includes a top-level `status` field:
- `"ok"` — full profile body.
- `"insufficient_corpus"` — partial body per §4.1.1, frontend shows the prompt to upload more.
- `"pending"` — row exists but job is queued/running (polled from `jobs` by join); frontend shows "Computing…".

404 only if no documents have ever been ingested AND no job is queued.

## Frontend tasks

### 4.5 Dashboard

`/personas/:id/dashboard`. Render the profile in sections using `StyleProfileCard` components.

Hero block:
- Persona name + relation + avatar.
- Chips: "{doc_count} documents · {chunk_count} chunks · {word_count} words".
- Era selector (tabs) — defaults to "All".

Sections (all card-based, no charts beyond sparklines):

1. **Vocabulary**
   - Big number: TTR.
   - Side stat: avg word length, vocabulary_level label.
   - Distinctive words: chip cloud (monospace chips, sizes proportional to score).
   - Characteristic phrases: list of bigrams/trigrams with counts.

2. **Rhythm & structure**
   - Avg sentence length + tiny histogram (custom SVG, minimal).
   - Sentence type mix: thin horizontal stacked bar.
   - Punctuation rhythm: grid of small stat cards.

3. **Themes**
   - Top topics: list with weight bars (max 8).
   - Recurring entities: chip list.

4. **Voice**
   - Opening gambits (list with "copy" button each).
   - Sign-offs.
   - Register label + first-person rate + contractions rate.

5. **Exemplars**
   - Cards showing 3–5 canonical chunks (truncated to 240 chars with "read more").
   - Each links to its source document.

"Recompute profile" button in the page header.

### 4.6 Empty state

If no documents yet → show a CTA linking to upload page and skip profile rendering.

## Quality bar

- On a 20 k word corpus, profile computation should produce a human-reviewable fingerprint that an outside reader could use to identify which of two writing samples is the persona.
- Sanity check during development: compute profiles for two obviously different personas (e.g. your 15-year-old essays vs recent formal writing). Verify metrics diverge noticeably — sentence length, vocabulary level, contractions rate, punctuation rhythm.

## Acceptance tests

1. After the first document finishes ingesting, a profile exists for the persona within 30 s.
2. With a corpus of only 800 tokens, the profile row has `status="insufficient_corpus"` and the dashboard renders the "upload more" hint (not a noisy fake profile).
3. Uploading enough docs to cross 2000 tokens triggers a recompute that yields a real `status="ok"` profile.
4. Creating a new era and tagging an existing document to it triggers a recompute; the era profile appears.
5. `POST /profile/recompute` twice in quick succession results in exactly one queued job (ON CONFLICT DO NOTHING).
6. Dashboard renders all sections without layout shift on 1440×900 and 1280×720 and at 375×812 (mobile).
7. Deleting all documents results in 404 on `/profile` and an empty-state dashboard.

## Out of scope

- Named entity recognition (beyond heuristic).
- Metaphor/figurative-language extraction.
- Comparing personas (v2 feature).
- Time-series of profile changes.
