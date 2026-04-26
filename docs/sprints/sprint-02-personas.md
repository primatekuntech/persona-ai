# Sprint 2 — Personas: create, edit, navigate

**Goal:** authenticated users can create, rename, and delete personas, define eras within a persona, and navigate into a per-persona workspace. No uploads yet.

**Duration estimate:** 3–4 days.

## Deliverables

1. Persona CRUD endpoints with strict user-scoping.
2. Era CRUD endpoints scoped to persona.
3. Cascade-delete behaviour: deleting a persona removes its rows **and** its files on disk (`/data/uploads/<persona_id>/**`, `/data/transcripts/<doc_id>.txt` for its documents, `/data/avatars/<persona_id>.*`).
4. Frontend: `/personas` list with create; `/personas/:id/dashboard` shell (empty); `/personas/:id/settings`; `/personas/:id/eras`.
5. Persona switcher in the top bar.
6. Avatar upload (optional; small images only).

Schema is already in the sprint-1 init migration per [`../02-data-model.md`](../02-data-model.md) — no new migrations this sprint.

## Backend tasks

### 2.1 Migrations

None. All persona/era schema lives in `20260425000000_init.sql` from sprint 1. This sprint is code-only against existing tables. If you need to add a column, create a new migration `20260426000000_personas_<thing>.sql` — do not edit the init.

### 2.2 Endpoints

```
GET    /api/personas                             list current user's personas (cursor pagination, default 50)
POST   /api/personas                             { name, relation?, description?, birth_year? } → persona   (idempotency-key required)
GET    /api/personas/:id                         one persona
PATCH  /api/personas/:id                         partial update
DELETE /api/personas/:id                         cascade delete (warn on frontend)

POST   /api/personas/:id/avatar                  multipart upload (image/png|jpeg), ≤ 2 MB → { avatar_path }
DELETE /api/personas/:id/avatar

GET    /api/personas/:id/eras                    list eras
POST   /api/personas/:id/eras                    { label, start_date?, end_date?, description? }
PATCH  /api/personas/:id/eras/:era_id
DELETE /api/personas/:id/eras/:era_id
```

All endpoints check `persona.user_id = ctx.user_id` before any operation. **404** (not 403) if mismatched — see [`../08-security.md`](../08-security.md#404-vs-403) for the rule. Idempotency key handling per [`../06-api-conventions.md`](../06-api-conventions.md#idempotency).

### 2.3 Validation

- `name`: 1–80 chars, unique per user.
- `relation`: one of `self | family | friend | other | null`.
- `birth_year`: 1900–current year if present.
- `era.label`: 1–40 chars, unique per persona.
- `era.start_date ≤ era.end_date` if both given.

Use `validator` crate or hand-rolled. Return `422 { error: { code: "validation_field", fields: { name: "..." } } }` for field-level semantic errors (per `docs/06-api-conventions.md` which uses 422 for semantic validation, not 400).

### 2.4 Avatars

- Store at `/data/avatars/<persona_id>.webp` — always webp after resize, so the extension is fixed.
- Validate mime via `infer` crate on the raw bytes, not the client-supplied `Content-Type`.
- Reject anything not `image/png`, `image/jpeg`, `image/webp`. Other image formats (gif, bmp, tiff, heic) return 415.
- Max 2 MB raw, resize to max 512×512 using the `image` crate, re-encode as webp (quality 85). This normalises output and strips EXIF (privacy).
- Serve via `GET /api/personas/:id/avatar` — streams from disk, authenticated, 404 if not found or not the caller's persona.

### 2.5 Cascade deletion

`DELETE /api/personas/:id` is a user-initiated destructive action. The transaction:

1. `SELECT ... FOR UPDATE` on `personas` row (guards concurrent deletes).
2. Gather `document_ids` for later filesystem cleanup.
3. `DELETE FROM personas WHERE id = $1 AND user_id = $2`. FK cascades handle `eras`, `documents`, `chunks`, `style_profiles`, `chat_sessions`, `chat_messages`, `jobs` rows (see [`../02-data-model.md`](../02-data-model.md#cascade-map)).
4. Commit.
5. **After** commit, asynchronously: delete `/data/uploads/<persona_id>/` (whole dir), `/data/transcripts/<doc_id>.txt` for each gathered doc id, and `/data/avatars/<persona_id>.webp`. Errors here are logged but do not fail the API response — the DB row is gone, so disk cleanup is a best-effort janitor task that a periodic filesystem sweeper (sprint 7) will catch up on.

Audit: `persona.deleted` with a count of removed documents/chunks in the details JSON.

### 2.6 Audit

Write audit events for `persona.created`, `persona.updated`, `persona.deleted`, `era.created`, `era.updated`, `era.deleted`.

## Frontend tasks

### 2.7 Personas list

`/personas` — grid of cards, each showing avatar, name, relation badge, `{doc_count} documents`, `{era_count} eras`, created date. "Create persona" primary button top-right opens a dialog.

Create dialog fields: name, relation (select), description (textarea), birth year (optional number). On submit → `POST /api/personas` → redirect to `/personas/:id/dashboard`.

### 2.8 Persona workspace shell

Route tree: `/personas/:id/*` wraps a layout with a persona sub-sidebar:

- Dashboard
- Documents (disabled until sprint 3)
- Chat (disabled until sprint 5)
- Eras
- Settings

Top bar swaps logo area for a **persona switcher** (combobox showing all user's personas; Cmd+P shortcut).

### 2.9 Dashboard (empty state for now)

- Persona name, relation, description, birth year.
- "Getting started" checklist:
  1. Create at least one era (optional).
  2. Upload documents _(disabled)_.
  3. Generate style profile _(auto, after uploads)_.
  4. Start a chat _(disabled)_.

### 2.10 Eras page

Table with Add/Edit/Delete. Era form fields per validation rules. Suggest label patterns like `age 13–16`, `2010–2012`.

If `birth_year` is set on the persona, offer an "Add age range…" helper that computes dates from ages entered.

### 2.11 Settings page

Edit name, description, relation, birth year, avatar. Danger zone: delete persona (confirms name re-entry).

## Acceptance tests

1. Two users each create a persona named "Alpha" — both succeed (names unique per user, not globally).
2. User A cannot GET user B's persona (**404**, not 403).
3. Deleting a persona also deletes its eras (FK cascade).
4. Deleting a persona with an avatar removes `/data/avatars/<id>.webp` within 5 s of the API response (async cleanup).
5. Avatar upload rejects a 5 MB jpeg with 413 (payload_too_large); accepts a 200 KB png, persists across reload, and the stored file is webp (content-sniffed, not filename-trusted).
6. Uploading a file with an `image/png` Content-Type header but GIF magic bytes is rejected with 415 (mime via `infer`).
7. Creating an era with `end_date < start_date` returns 422 with `fields.end_date` set (semantic validation per `docs/06-api-conventions.md`).
8. Cmd+P opens the persona switcher and filters by typed text.
9. POST `/api/personas` twice with the same `Idempotency-Key` creates one row and returns the same persona both times.
10. Two concurrent DELETE requests on the same persona: first returns 204, second returns 404 (no orphaned disk state).

## Out of scope

- Documents, ingestion, analysis, chat, export.
- Persona sharing or export.
