# Data lifecycle

How data enters, lives in, and leaves the system. This is the stance on deletion, retention, portability, and disk management for a private, invite-only product.

## Principles

1. **Users own their data.** They can export everything they've put in, and they can delete it on request.
2. **Deletion is real.** "Deleted" means gone from database, filesystem, and backups within a bounded window — not a soft flag.
3. **Retention is explicit.** Every class of data has a stated lifetime.
4. **Quotas prevent surprise.** No single user can fill the VPS by accident or malice.

## Data classes

| Class | Storage | Owner | Default retention | Backup |
|-------|---------|-------|-------------------|--------|
| Account (users, sessions) | Postgres | User | Until deletion | Yes |
| Personas / eras | Postgres | User | Until deletion | Yes |
| Documents (originals) | Filesystem | User | Until deletion | Yes |
| Transcripts | Filesystem | User | Until deletion | Yes |
| Chunks + embeddings | Postgres | User | Until deletion | Yes |
| Style profiles | Postgres | User | Until deletion | Yes |
| Chat sessions + messages | Postgres | User | Until deletion | Yes |
| Exports (.md / .docx) | Filesystem | User | 30 days | No |
| Audit log | Postgres | System | 180 days | Yes |
| Jobs (queue) | Postgres | System | 30 days after finish | No |
| Idempotency keys | Postgres | System | 24 hours | No |
| Model files | Filesystem | Admin | Until replaced | No (re-downloadable) |
| HTTP/tracing logs | Stdout → journald | System | 14 days | No |

## User account lifecycle

### Creation

- Admin issues an invite (new or existing email).
- If email matches an existing active user, reject with `409 conflict` (`code: "user_exists"`). Inviter can resend a password reset instead.
- If email matches a **disabled** user, reject — admin must re-enable or delete first.
- Invitee accepts within 7 days, sets password, becomes `status = 'active'`.

### Disable

- Admin can disable a user (`status = 'disabled'`).
- Disabled users cannot log in. All of their sessions are invalidated immediately (delete all rows in `sessions` where `user_id = X`).
- **Disable does not delete data.** A disabled user's personas, documents, chats remain in the database.
- Use-case: temporary access removal. Disable is reversible; deletion is not.

### Delete (hard)

- Admin can delete a user.
- Confirmation gate: admin retypes the target email.
- Effect: cascades through personas → eras → documents → chunks → chat_sessions → messages via `ON DELETE CASCADE`. Sessions, password_resets, audit_log entries (null the `user_id`, keep the row) also clean up. See "Audit log survival" below.
- **Filesystem cleanup is not automatic.** A transactional job is enqueued on the same commit: deletes the user's upload/transcript/avatar/export directories. The job retries with backoff; eventual consistency within 1 hour.
- Deletion is logged in `audit_log` with `user_id = NULL, action = 'user.deleted', metadata = {email, deleted_by}`. Post-deletion the audit row no longer identifies a living user but retains the admin's record of action.

### Self-delete

Users can request deletion of their own account from `/settings/account`. The UI shows a two-step confirmation, enumerates what will be deleted, and requires re-entering the password. Upon confirmation, the flow is identical to admin deletion. Admins cannot prevent a user from deleting themselves; the only enforcement is preventing the **last admin** from self-deleting (app returns 409 `last_admin`).

### Data export (portability)

A user can request an export of all their data:

- Endpoint: `POST /api/auth/export` → enqueues a `user_export` job.
- Output: a zip archive under `/data/exports/<user_id>/<ts>.zip` with:
  - `profile.json` — user account details (no password).
  - `personas/<persona_slug>/`
    - `persona.json`
    - `eras.json`
    - `documents/<title>.<ext>` — original files
    - `transcripts/<doc_id>.txt`
    - `style_profile.json`
    - `eras/<era_slug>/style_profile.json`
    - `chats/<session_id>.md` — rendered conversation
  - `README.txt` — explaining the structure.
- Zip available via a signed, time-limited download URL.
- Export retained 7 days then auto-deleted.
- Rate-limited to 1 export per 24 hours per user.

## Persona lifecycle

Deleting a persona cascades to its eras, documents, chunks, chat sessions, messages, and style profiles. File-system cleanup is enqueued as above (`/data/uploads/<persona_id>/`, `/data/avatars/<persona_id>*`).

Before the actual delete, the UI shows counts: "This will delete 42 documents, 3 eras, 12 chat sessions. Type the persona name to confirm."

## Document lifecycle

- Upload → ingested → chunks persisted → profile recomputed.
- User can delete a document. Cascades to chunks. Originals and transcripts are removed synchronously (small, one directory entry each).
- User can re-ingest (re-parse, re-chunk, re-embed). Chunks are replaced; profile is re-queued.
- **Duplicate detection:** every upload computes `sha256` of the file bytes. If a document with the same hash exists for the same `(user_id, persona_id)`, return 409 `duplicate_document` with the existing `document_id`. Hash stored as `documents.content_hash` (indexed).

### Incremental profile recompute

Full rebuild is the v1 approach. For corpora > 200 k words it becomes slow (> 30 s). v2 plan noted here:

- Maintain per-metric running aggregates (word counts, n-gram counts, sentence-length histograms) incrementally on chunk insert/delete.
- Profile JSON is then a cheap snapshot of the aggregates.

Not implemented in v1. Specified so future work has a home.

## Chat lifecycle

- Chat sessions are retained until user deletes them or deletes their account.
- Messages in deleted sessions are deleted with the session.
- **No automatic expiry.** Users can archive (soft delete into a hidden list) or hard delete. v1 only has hard delete.

## Export files

Generated `.md` / `.docx` live in `/data/exports/<user_id>/`. Pruned nightly: files older than 30 days deleted. The `user_export` zips follow the same rule with a tighter 7-day window.

## Audit log

### Retention

- Default: 180 days.
- Admin-configurable via `AUDIT_RETENTION_DAYS` env var; minimum 30, maximum 1825.
- Nightly job deletes rows where `created_at < now() - interval '180 days'`.

### Partitioning (when it matters)

Not in v1. When table exceeds 10 million rows, switch to monthly partitions and drop whole partitions on schedule. Migration path documented here; execution deferred.

### Survival through user deletion

When a user is deleted, their audit rows are **not** deleted. Instead:

- `user_id` column is nullable; on user delete the column is nulled.
- A `metadata.deleted_user_email` field is stored on the deleted row so admins can still investigate actions.
- This is documented in the admin UI ("audit log is retained for 180 days, even after user deletion").

## Queue & idempotency tables

- `jobs` rows in `done` status retained 30 days then deleted nightly.
- `jobs` rows in `failed` status retained 180 days (same as audit log) so operators can investigate.
- `idempotency_keys` retained 24 hours.

## Storage quotas

### Per-user quotas

Each user has three quota limits:

| Quota | Default | Column |
|-------|---------|--------|
| Total storage (uploads + transcripts) | 10 GB | `users.quota_storage_bytes` |
| Total documents | 2000 | soft; computed |
| Total personas | 25 | soft; computed |

Admin can raise per-user via `PATCH /api/admin/users/:id { quota_storage_bytes: ... }`.

### Enforcement

- On upload, before the multipart stream starts, check a rough current usage. Reject with 413 `quota_exceeded` if `current_usage + content_length > quota`.
- Current usage recomputed lazily: store `users.current_storage_bytes`; update in the same transaction as document insert/delete. A nightly reconciler recomputes from scratch to catch drift.

### VPS-wide protection

Separate from per-user quotas, a global watchdog checks `df` on `/data` every 5 minutes. At > 85 % full, the backend refuses new uploads globally with 503 `dependency_unavailable` (message: "server out of disk; contact admin"). Admin receives an email alert via Resend.

## Backups

### Scope

- Postgres: `pg_dump -Fc` nightly → `/data/backups/db/persona-<date>.dump`.
- Filesystem user data: nightly `rsync` of `/data/uploads`, `/data/transcripts`, `/data/avatars` to off-site.
- Excluded from backup: `/data/exports/*` (regeneratable), `/data/models/*` (re-downloadable), logs.

### Retention

- Daily: last 30 days.
- Monthly: last 12 months (first daily of each month promoted to monthly).

### Off-site

Off-site destination configured via `BACKUP_DESTINATION` env (rsync target). Optional `BACKUP_ENCRYPTION_KEY` wraps archives with `age` before upload.

### Restore test

Quarterly manual restore to a disposable VPS; document the runbook in [`sprints/sprint-07-polish-deploy.md`](sprints/sprint-07-polish-deploy.md).

### User-visible backups

Not a product feature in v1. If a user deletes something they didn't mean to, they must ask the admin to restore from backup (manually). Communicate this in the confirmation dialogs ("this cannot be undone from within the app").

## GDPR-style rights (posture)

Even though this is a private app, we provide the core rights because they are cheap and correct:

- **Right to access:** `POST /api/auth/export` (see above).
- **Right to erasure:** self-delete from `/settings/account`.
- **Right to rectification:** `PATCH /api/auth/me` for profile fields.
- **Right to data portability:** the export is open-format (JSON + original files + markdown chats).

We are not claiming regulatory compliance — we are matching the substance.

## Copyright on generated outputs

Generated text is produced from a local LLM running on the user's own infrastructure, using the user's own corpus. We claim no rights over outputs. The user is responsible for the use of outputs that reference third parties (living or deceased) whose writing they did not own. This note appears in the app under `/settings/about`.

## Analytics

**No analytics.** No page-view tracking, no event pipeline, no telemetry leaves the VPS. Logs stay on the server. This is a deliberate product stance, stated here so it isn't silently reversed later.

## Decisions

These are explicit product decisions worth preserving:

- **Soft delete is forbidden.** Data users delete is deleted. If we need undo, we rely on backups, not a `deleted_at` flag.
- **No sharing.** Personas are never shared between users. `user_id` scoping is an invariant, not a feature.
- **No public sign-up.** Invite-only. Removing this is a major product change, not a config flag.
- **No auto-archival.** Data ages in place. If we ever add auto-delete of old chats, it requires explicit user opt-in.
