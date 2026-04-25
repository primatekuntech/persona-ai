# CLAUDE.md

Instructions for Claude (or any LLM pair-programmer) working in this repo.

## Project posture

- This is a greenfield project; planning is complete, code is not started.
- All design decisions live in [`docs/`](docs/). The sprint docs (`docs/sprints/`) are the **source of truth for what to build**. If code diverges from the sprint doc, update the doc in the same change.
- One user, one VPS, CPU-only, no external LLM calls. Privacy is load-bearing.
- No feature flags, no A/B, no analytics. If you catch yourself reaching for one, stop and re-read [`docs/00-overview.md`](docs/00-overview.md#explicitly-deferred-features-not-v1).

## Where to look first

Before writing any backend code, open:

1. [`docs/01-architecture.md`](docs/01-architecture.md) — runtime model, RAM budget, concurrency knobs.
2. [`docs/02-data-model.md`](docs/02-data-model.md) — every table, index, and FK. All schema already lives in one init migration; do not add per-sprint DDL unless the feature is genuinely new.
3. [`docs/06-api-conventions.md`](docs/06-api-conventions.md) — error envelope, cursor pagination, idempotency, SSE rules.
4. [`docs/08-security.md`](docs/08-security.md) — CSRF, brute-force, upload security, prompt injection defences.

Before writing any frontend code:

1. [`docs/03-design-system.md`](docs/03-design-system.md) — colours (CSS variables), typography, components, mobile shell, keyboard shortcuts.
2. [`docs/06-api-conventions.md`](docs/06-api-conventions.md#sse) — SSE streams use `fetch + ReadableStream` (not `EventSource`) because they're POST requests.

## Rules that bite

- **Every row in a domain table carries `user_id`.** Every repository function takes `user_id`. Every query filters on it. No exceptions, no "admin bypass" (there's no admin bypass — see [`docs/08-security.md`](docs/08-security.md#admin-impersonation-forbidden)).
- **404 not 403** when hiding cross-user data. 403 is only for authenticated-but-insufficient-role cases (e.g. non-admin hitting `/admin/*`). See [`docs/08-security.md`](docs/08-security.md#404-vs-403).
- **Roles are not cached in the session.** `require_auth` refetches `users.role` on every request. Don't copy role into session data.
- **No `unwrap()` in production code.** Tests may use it. Justify every `expect()`.
- **No SQL strings with user input concatenated in.** `sqlx` macros or `$1`-parameterised queries only.
- **TDD for:** auth boundaries, repositories that scope by `user_id`, parsers, analysers, prompt builder, error mapping. See [`docs/05-engineering-practices.md`](docs/05-engineering-practices.md#where-tdd-is-non-negotiable).
- **Validation parity:** backend `validator` + `serde(deny_unknown_fields)` is the source of truth. Frontend `zod` duplicates rules for UX. If you change a rule, update both in the same change.
- **Blocking work off the runtime.** Whisper / llama / fastembed are CPU-bound — always inside `tokio::task::spawn_blocking` behind their respective semaphores.

## Commit style

- Conventional commits optional but consistent: `feat:`, `fix:`, `chore:`, `refactor:`, `test:`, `docs:`, `deps:`.
- Imperative mood, < 72-char subject, body wrapped at 100.
- One logical change per commit. Don't bundle refactors with features.

## When asked to implement a sprint

1. Re-read the sprint doc top to bottom first. The doc is the spec.
2. If you find a gap or ambiguity, **update the sprint doc first**, then write code against the updated spec.
3. Follow the TDD rhythm from [`docs/05-engineering-practices.md`](docs/05-engineering-practices.md#default-rhythm) for everything in the TDD-non-negotiable list.
4. Tick the sprint's Acceptance tests as you go — they double as the PR's done-definition.

## Things not to do

- Do not invent features that aren't in a sprint doc.
- Do not add `GET /api/v1/*` prefixes — the API is unversioned (see [`docs/06-api-conventions.md`](docs/06-api-conventions.md#versioning)).
- Do not serialize enums via `#[serde(untagged)]` or `#[serde(rename_all = "camelCase")]` unless the spec says so (API is snake_case).
- Do not reach for an ORM. `sqlx` with typed queries is enough.
- Do not add a JS state-management library beyond TanStack Query + Zustand.
- Do not soft-delete anything. `DELETE` means gone.
- Do not cache role, permissions, or quotas in the session. Fetch them fresh per request.
