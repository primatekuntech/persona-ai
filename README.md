# Persona AI

A private, local-first RAG system that builds a **mimic persona** of a real person (often yourself at an earlier age), ingests their writing and speech, extracts a style profile, and generates new text in their voice — all running on a single CPU-only VPS.

No third-party LLM calls by default. No telemetry. Invite-only. Multilingual (Bahasa Malaysia, English, Mandarin, Cantonese, Tamil and more).

## Status

**All 8 sprints complete** as of 2026-04-27. The app is production-ready.

The design lives in [`docs/`](docs/). Start at [`docs/00-overview.md`](docs/00-overview.md) for the vision, or jump to [`docs/runbook.md`](docs/runbook.md) to deploy.

## Stack at a glance

- **Backend:** Rust (axum + tokio + sqlx), Postgres 16 + pgvector, `whisper-rs` for STT (Whisper large-v3, multilingual), `fastembed` for embeddings (bge-m3, 100+ languages), `llama-cpp-2` for local LLM (Qwen2.5-7B).
- **Frontend:** React 18 + Vite + TypeScript, Tailwind + shadcn/ui, TanStack Query + Zustand.
- **Infra:** Podman + Caddy (auto-HTTPS) on a 16 GB / 4 vCPU VPS.
- **Email:** Resend for invites and password resets.
- **Cloud opt-in:** users can add OpenAI-compatible endpoints or Google Speech via Settings → Integrations; local models remain the default.

Full tech stack in [`docs/01-architecture.md`](docs/01-architecture.md). Model choices in [`docs/04-models.md`](docs/04-models.md).

## Repo layout

```
.
├── backend/      # Rust binary: axum HTTP, workers, repositories, model runtimes
├── frontend/     # Vite + React SPA
├── docker/       # Dockerfiles + Caddyfile
├── docs/         # Specs, sprints, reference material
├── scripts/      # download-models.sh, backup helpers
└── .github/      # CI workflows
```

## Quick start (development)

```bash
# Backend
cd backend
cargo run

# Frontend (separate terminal)
cd frontend
npm install
npm run dev
```

See [`docs/runbook.md`](docs/runbook.md) for production deployment with Podman.

## Getting involved

This is a single-operator product. If you've been invited, see `/settings/about` inside the app for the privacy posture. The source itself is private during v1.

## Licence

See [`LICENSE`](LICENSE).
