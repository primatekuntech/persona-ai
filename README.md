# Persona AI

A private, local-first RAG system that builds a **mimic persona** of a real person (often yourself at an earlier age), ingests their writing and speech, extracts a style profile, and generates new text in their voice — all running on a single CPU-only VPS.

No third-party LLM calls. No telemetry. Invite-only.

## Status

**Planning + spec review complete** as of 2026-04-25. Code is not yet started.

The design lives in [`docs/`](docs/). Start at [`docs/00-overview.md`](docs/00-overview.md) for the vision, then [`docs/sprints/sprint-01-foundation.md`](docs/sprints/sprint-01-foundation.md) for the first executable slice.

## Stack at a glance

- **Backend:** Rust (axum + tokio + sqlx), Postgres 16 + pgvector, `whisper-rs` for STT, `fastembed` for embeddings, `llama-cpp-2` for local LLM.
- **Frontend:** React 18 + Vite + TypeScript, Tailwind + shadcn/ui, TanStack Query + Zustand.
- **Infra:** Docker + Caddy (auto-HTTPS) on a 16 GB / 4 vCPU VPS.
- **Email:** Resend for invites and password resets.

Full tech stack in [`docs/01-architecture.md`](docs/01-architecture.md). Model choices in [`docs/04-models.md`](docs/04-models.md).

## Repo layout (once scaffolded in sprint 1)

```
.
├── backend/      # Rust binary: axum HTTP, workers, repositories, model runtimes
├── frontend/    # Vite + React SPA
├── docker/      # Dockerfiles + Caddyfile
├── docs/        # Specs, sprints, reference material
├── scripts/     # download-models.sh, backup helpers
└── .github/     # CI workflows
```

## Getting involved

This is a single-operator product. If you've been invited, see `/settings/about` inside the app for the privacy posture. The source itself is private during v1.

## Licence

See [`LICENSE`](LICENSE).
