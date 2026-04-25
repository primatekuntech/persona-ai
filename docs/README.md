# Persona AI — Documentation

Specs and execution plan for the Persona AI system. Read in order if new; jump around if you know what you need.

## Read first

1. [`00-overview.md`](00-overview.md) — vision, glossary, primary user flow, explicitly deferred features.
2. [`01-architecture.md`](01-architecture.md) — components, tech stack, runtime model, RAM budget, error tracking.
3. [`02-data-model.md`](02-data-model.md) — Postgres schema (including pgvector, quotas, idempotency, errors).
4. [`03-design-system.md`](03-design-system.md) — modern, minimalist visual language (with mobile shell and keyboard shortcuts).
5. [`04-models.md`](04-models.md) — which AI models to use for each task, plus SHA-256 integrity verification.
6. [`05-engineering-practices.md`](05-engineering-practices.md) — TDD, SOLID, DRY, CI, axe-core, validation parity.
7. [`06-api-conventions.md`](06-api-conventions.md) — error envelope, cursor pagination, idempotency keys, SSE rules, rate-limit scopes.
8. [`07-data-lifecycle.md`](07-data-lifecycle.md) — retention, deletion, user self-delete, per-user quotas, exports, backups, copyright stance.
9. [`08-security.md`](08-security.md) — threat model, password hashing, session, CSRF, brute-force, upload security, prompt injection, headers.

## Execution plan — sprints

Each sprint is a shippable vertical slice. Order matters; later sprints depend on earlier ones.

| # | Doc | Scope |
|---|-----|-------|
| 1 | [`sprints/sprint-01-foundation.md`](sprints/sprint-01-foundation.md) | Repo scaffold, Postgres, auth, invite-only multi-user, Resend email. |
| 2 | [`sprints/sprint-02-personas.md`](sprints/sprint-02-personas.md) | Persona and Era CRUD, persona workspace shell, switcher. |
| 3 | [`sprints/sprint-03-ingestion.md`](sprints/sprint-03-ingestion.md) | Upload, Whisper transcription, chunking, embedding, job queue. |
| 4 | [`sprints/sprint-04-analysis.md`](sprints/sprint-04-analysis.md) | Per-persona style profile (lexical, syntactic, semantic, stylistic). |
| 5 | [`sprints/sprint-05-chat-rag.md`](sprints/sprint-05-chat-rag.md) | Hybrid retrieval, persona prompt, local LLM with SSE streaming. |
| 6 | [`sprints/sprint-06-export.md`](sprints/sprint-06-export.md) | Export chat to `.md` and `.docx`. |
| 7 | [`sprints/sprint-07-polish-deploy.md`](sprints/sprint-07-polish-deploy.md) | Hardening, HTTPS, backups, VPS runbook. |

## Reference

- [`reference/training-methodology.md`](reference/training-methodology.md) — how per-persona "training" works on CPU, why the approach is sound, its limits, and the GPU upgrade path.

## Conventions used across docs

- **Persona** is a first-class entity. One user owns many personas. Each persona has its own corpus, style profile, and chats.
- **Era** is an optional time window within a persona, used to filter retrieval and segment analysis.
- Every row in a domain table carries `user_id`. No query bypasses it.
- Code samples are illustrative, not final APIs — specs specify *behaviour*, not exact signatures.
- "v1" / "v2" tags mark scope. v1 = what we ship first; v2 = a deliberate later increment.

## Status

Planning + review complete as of 2026-04-25. Code not started. Next step: scaffold per [`sprints/sprint-01-foundation.md`](sprints/sprint-01-foundation.md).

### Changes since the initial spec pass

- Added `06-api-conventions.md`, `07-data-lifecycle.md`, `08-security.md`.
- Init migration now carries the full schema (identity + personas + documents + chunks + jobs + profiles + chats + errors + idempotency + audit + login_attempts); sprint docs beyond sprint 1 are code-only.
- Per-user storage / document / persona quotas enforced at upload time.
- Duplicate-document detection by content hash.
- Stuck-job reaper with worker heartbeats.
- Idempotency-key support on all state-changing POSTs.
- Prompt-injection defences layered into retrieval + prompt build + output filter.
- Context-window eviction algorithm spelled out.
- CSRF via double-submit cookie; login password-reset self-service.
- Model-file SHA-256 verification on startup.
- Corpus-token floor (2000) before generating a style profile.
- Mobile-first shell + keyboard shortcuts in the design system.
- Admin impersonation forbidden; read-only admin views for debugging.
- Daily filesystem/table cleanup job.
- Self-service full-account export (`.zip`) and account deletion in sprint 7.
- Per-user + server-wide generation concurrency caps with FIFO queue and 20 s timeout.
- `jobs` table carries `user_id`, `persona_id`, `worker_id`, `heartbeat_at` columns; reaper thresholds on heartbeat, not `started_at`.
