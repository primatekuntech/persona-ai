# Persona RAG — Overview

## Vision

A private, local-first system that lets a user build a **mimic persona** of a real person (often themselves at an earlier age), feed it that person's writing and speech, and then generate new text that reproduces how that person thought and wrote.

The system is invite-only, runs entirely on the user's own infrastructure, and never sends corpus data to a third-party LLM. Every component — speech-to-text, embeddings, language model — runs locally on CPU.

## Who it's for

A single VPS owner (the admin) who invites a small group of trusted users. Each user builds their own set of mimic personas in isolation; no user can see another's data.

## Primary user flow

```
1. Admin invites user by email (Resend)
        ↓
2. User accepts invite, sets password, logs in
        ↓
3. User creates a Persona  ──────────► e.g. "Me, age 15" or "Grandfather (1970s)"
        ↓
4. User uploads that Persona's documents     (essays, journals, letters, recordings)
        ↓
5. System ingests, transcribes, chunks, embeds, and analyses
        ↓
6. System produces a per-Persona Style Profile and surface-level "dashboard"
        ↓
7. User chats with the Persona from a selected Era
        ↓
8. User exports generated writing as .md or .docx
```

Creating additional personas repeats steps 3–8. Each persona is self-contained: its own corpus, its own profile, its own chats.

## What "mimicry" means here

We are **not** fine-tuning model weights per persona. On a CPU-only VPS, weight updates are impractical. Instead, mimicry is achieved through:

1. A **Style Profile** extracted offline from the persona's corpus (vocabulary, sentence rhythm, recurring phrases, topical tendencies, era segmentation).
2. **Retrieval** of the closest stylistic exemplars from the persona's own writing at query time.
3. A **persona prompt** that instructs the LLM to match the profile, grounded in the retrieved exemplars.

This reliably captures surface style (word choice, rhythm, imagery, opening gambits, sign-offs) and topical concerns. It does not transfer deep personality or reasoning patterns the same way a fine-tuned model would. See `reference/training-methodology.md` for the full rationale and the optional GPU upgrade path.

## Glossary

| Term | Meaning |
|------|---------|
| **User** | An authenticated account holder. Owns personas and chats. |
| **Admin** | A user with role `admin`. Can create invites and manage users. |
| **Persona** | A mimic target owned by one user. Has a name, optional relation (self/other), description, and avatar. Scopes a corpus and a style profile. |
| **Era** | A named time window within a persona (e.g. "age 13–16"). Optional. Used to filter retrieval and segment the profile. |
| **Document** | A single uploaded artifact: a `.txt`, `.md`, `.docx`, `.pdf`, `.mp3`, `.wav`, or `.m4a`. Belongs to one persona, optionally one era. |
| **Chunk** | A ~400-token slice of a document. Carries an embedding and metadata. The retrieval unit. |
| **Style Profile** | A JSON document, recomputed after ingestion, summarising lexical, syntactic, semantic, and stylistic features of a persona's corpus. |
| **Exemplar** | A chunk retrieved at query time to ground generation. |
| **Session** | A chat conversation with one persona (optionally one era). Holds message history. |

## What this system is **not**

- Not a general-purpose chatbot. It only speaks as one of the user's personas.
- Not a multi-tenant SaaS. Built for a single VPS, a handful of trusted users.
- Not a fine-tuning platform. No GPU, no weight updates in v1.
- Not a social network. No sharing of personas between users.
- Not a publishing tool. Export produces private documents; distribution is the user's problem.
- Not multilingual in v1 — English only. See [`04-models.md`](04-models.md) for the v2 upgrade path.

## Ownership of outputs

Generated text is produced locally from the user's own corpus on the user's own VPS. We claim no rights to outputs. The user is solely responsible for how they use text that imitates third parties (living or deceased) whose writing they did not own. This posture is restated in [`07-data-lifecycle.md`](07-data-lifecycle.md#copyright-on-generated-outputs).

## Explicitly deferred features (not v1)

Each of these has been considered and deferred. Documented here so they are not accidentally re-invented:

- **Self-service MFA (TOTP).** Stubbed in [`08-security.md`](08-security.md#multi-factor-authentication); v2.
- **Per-persona LoRA fine-tuning.** Requires GPU; [`reference/training-methodology.md`](reference/training-methodology.md#upgrade-path--when-gpu-becomes-available).
- **Analytics / telemetry.** Deliberately excluded — see [`07-data-lifecycle.md`](07-data-lifecycle.md#analytics).
- **Visual regression testing, load testing.** Valuable but not before first ship.
- **Persona sharing, public signup, federation.** Intentional product boundary.
- **OpenAPI spec.** Sprint docs are the contract; revisit if third-party clients appear.
- **Feature flags, A/B testing.** Not worth the infrastructure at this scale.
- **Formal ToS / privacy policy.** Honest `/settings/about` page serves the purpose for invited users.
- **PDF export, URL import, Twitter archive import, OCR.** Scope grows v2.
- **Regenerate / branch chat messages, message feedback (thumbs).** v2.

## Success criteria (v1)

- Admin can invite a user, user can log in.
- A user can create a persona, upload ≥ 10 documents, see them ingest to completion.
- The persona dashboard shows meaningful style metrics.
- Chat generates responses that a reader who knows the persona's corpus recognises as "in their voice".
- Response export produces a readable `.docx`.
- End-to-end, runs on a 16 GB RAM VPS with no external LLM calls.

## Related documents

- [`01-architecture.md`](01-architecture.md) — components, technologies, runtime model.
- [`02-data-model.md`](02-data-model.md) — Postgres schema including pgvector.
- [`03-design-system.md`](03-design-system.md) — visual language.
- [`04-models.md`](04-models.md) — recommended local models per task.
- [`05-engineering-practices.md`](05-engineering-practices.md) — TDD, SOLID, DRY, CI.
- [`06-api-conventions.md`](06-api-conventions.md) — HTTP request/response rules.
- [`07-data-lifecycle.md`](07-data-lifecycle.md) — deletion, retention, quotas, portability.
- [`08-security.md`](08-security.md) — threat model and defences.
- `sprints/` — execution plan, seven sprints from scaffold to deploy.
- `reference/training-methodology.md` — why the mimicry approach works and its limits.
