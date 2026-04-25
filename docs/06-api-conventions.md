# API conventions

Shared rules for every HTTP endpoint in this project. Sprint docs describe *what* an endpoint does; this doc describes *how* it shapes its request and response.

## Versioning

**No version prefix.** All routes live under `/api/*`, not `/api/v1/*`.

Rationale: single-tenant private app, the client ships with the server, there is no third-party consumer to maintain compatibility with. Breaking changes are coordinated by deploying frontend and backend together.

If this assumption ever breaks (third-party API consumers appear), freeze `/api/*` as v1 and introduce `/api/v2/*` alongside. Do not retrofit versioning prematurely.

## Content types

- **Request bodies:** `application/json; charset=utf-8` unless the endpoint accepts file uploads, in which case `multipart/form-data`.
- **Response bodies:** `application/json; charset=utf-8` for all JSON endpoints.
- **Streaming:** `text/event-stream; charset=utf-8` for SSE endpoints.
- **Downloads:** endpoint-specific (`text/markdown`, `application/vnd.openxmlformats-officedocument.wordprocessingml.document`, etc.) with `Content-Disposition: attachment; filename="..."`.

Reject requests with unexpected `Content-Type` with HTTP 415.

## Status codes

| Code | Use |
|------|-----|
| 200 | Success with body. |
| 201 | Resource created; response body is the created resource. |
| 204 | Success with no body. |
| 400 | Validation or malformed input. |
| 401 | No session or expired. |
| 403 | Authenticated but not permitted. |
| 404 | Resource not found **or** resource exists but not visible to this user (do not distinguish — avoid leaking existence). |
| 409 | Conflict (duplicate unique key, concurrent modification). |
| 413 | Payload too large. |
| 415 | Unsupported media type. |
| 422 | Semantic validation error (valid JSON, invalid values). Reserved for field-level errors. |
| 429 | Rate limited. |
| 500 | Internal error. |
| 503 | Dependency unavailable (DB down, model not loaded). |

We use **400 for syntactic** (bad JSON, missing field, wrong type) and **422 for semantic** (field present but value violates business rule). Some specs use 400 loosely; 422 is more correct for field-level validation, and the client distinguishes them.

## Error envelope

Every non-success response body:

```json
{
  "error": {
    "code": "<machine_code>",
    "message": "<human safe message>",
    "fields": { "<field>": "<message>" }        // only on 422
  },
  "request_id": "<uuid>"
}
```

`request_id` matches the `X-Request-ID` header (generated server-side per request; clients may supply one to be echoed). It appears in logs and in toast messages so users can quote it when reporting issues.

`message` is safe to show to users — no stack traces, no internal paths, no SQL.

### Standard error codes

Canonical values for `error.code`. Use these; don't invent synonyms.

| Code | Status | Meaning |
|------|--------|---------|
| `validation` | 400 | Request malformed. |
| `validation_field` | 422 | Field-level validation failed; see `fields`. |
| `unauthorized` | 401 | No session. |
| `forbidden` | 403 | Wrong role. |
| `not_found` | 404 | Resource absent or not visible. |
| `conflict` | 409 | Duplicate or concurrent update. |
| `payload_too_large` | 413 | Request body exceeded limit. |
| `unsupported_media_type` | 415 | Wrong `Content-Type`. |
| `rate_limited` | 429 | See `Retry-After`. |
| `internal` | 500 | Server bug — request_id is the lead. |
| `dependency_unavailable` | 503 | DB/model/external service down. |
| `quota_exceeded` | 413 | User-level quota reached; distinct from body size. |
| `llm_busy` | 503 | Generation slot unavailable; retry soon. |
| `server_busy` | 503 | All generation permits in use, wait queue exceeded 20 s. |
| `generation_concurrency_exceeded` | 429 | User has too many in-flight streams (per-user cap). |
| `ingest_failed` | 422 | Document failed to ingest; see `fields.reason`. |
| `audio_too_long` | 413 | Audio upload exceeds the per-file duration cap. |
| `corpus_too_small` | 422 | Persona corpus is below the 2000-token floor for profile build. |
| `invalid_token` | 400 | Invite/reset token invalid or expired. |
| `last_admin` | 409 | Cannot demote or self-delete the sole remaining admin. |
| `csrf_failed` | 403 | Missing or mismatched `X-CSRF-Token`. |

## Pagination

Cursor-based. Never offset-based — offset pagination is wrong under concurrent writes.

### Request

```
GET /api/personas/:id/documents?limit=50&cursor=<opaque>
```

- `limit`: 1–200, default 50.
- `cursor`: opaque base64-url string; absent on first page.

### Response

```json
{
  "items": [ ... ],
  "next_cursor": "<opaque or null>",
  "total_estimate": 1243   // optional; omit if expensive to compute
}
```

- `next_cursor: null` signals the last page.
- `total_estimate` is optional and explicitly *estimate*; we do not promise exact counts.

### Implementation

The cursor encodes `(sort_key, id)` for deterministic pagination. For `documents`, the default sort is `(-created_at, id)`; the cursor is base64(`<iso_ts>|<uuid>`). Keyset query:

```sql
WHERE user_id = $1
  AND (created_at, id) < ($cursor_ts, $cursor_id)
ORDER BY created_at DESC, id DESC
LIMIT $limit + 1;
```

Peek at `$limit + 1` to know if another page exists.

## Filtering and sorting

Filters are query-string parameters named after the field: `?status=done&era_id=<uuid>`. Multi-valued filters repeat the key: `?status=done&status=failed`.

Sorting is `?sort=-created_at` (prefix `-` for descending). Only whitelisted sort fields per endpoint; reject unknown with 422.

## Idempotency

Any `POST` that creates a resource **must** accept an optional `Idempotency-Key` header (client-generated UUID). Semantics:

- If the server has seen this key in the last 24 hours with the same `(user_id, route)`, return the original response (200/201) instead of re-executing.
- If seen with a different body, return `409 conflict`.
- Stored in a small table `idempotency_keys(key, user_id, route, response_status, response_body, created_at)`; purge rows older than 24 h via a daily job.

Required for: document upload, chat message send, invite creation, export generation. Safe to omit on reads and idempotent updates (PATCH with identical payload is already idempotent).

## Request IDs

Every request has `X-Request-ID`. If the client supplies one, validate as UUID and echo it; else generate. Log with every span. Include in every error envelope.

## Authentication

Session cookie (`pai_session`) is the only auth mechanism. Set with `HttpOnly; Secure; SameSite=Lax; Path=/`. No Bearer tokens, no API keys. Clients send credentials with every request: `fetch(url, { credentials: "include" })`.

## CSRF

See [`08-security.md`](08-security.md#csrf). Summary:

- Double-submit cookie pattern.
- `pai_csrf` cookie (non-HttpOnly, `Secure`, `SameSite=Lax`) set at login; rotated on every response.
- Header `X-CSRF-Token` must match cookie on every non-GET/HEAD/OPTIONS request.
- Exempt: `/api/auth/login`, `/api/invites/accept` (pre-session endpoints).

## Rate limiting

Scopes and defaults:

| Scope | Limit | Scope-key |
|-------|-------|-----------|
| Unauthenticated auth endpoints | 10 req / 60 s | IP |
| Authenticated default | 600 req / 60 s | user_id |
| Uploads | 60 req / 60 min | user_id |
| Generation (chat message) | 30 req / 60 min | user_id |
| Admin invite create | 30 req / 60 min | user_id |
| Account export | 1 req / 24 h | user_id |
| Account self-delete | 3 req / 24 h | user_id |

Response on limit: 429 with `Retry-After: <seconds>` and body `{error:{code:"rate_limited", message:"..."}}`.

## Timestamps

All timestamps in API payloads are ISO 8601 with timezone: `"2026-04-25T14:22:33.123Z"`. Storage is `TIMESTAMPTZ`. Clients format in the user's locale; server never returns pre-formatted strings.

## IDs

All IDs are UUID v4 (unless noted). Serialised as lowercase hyphenated strings. Never expose internal BIGSERIAL ids to the API (only `audit_log.id` is BIGSERIAL and we don't expose it directly).

## Validation

- Backend: `validator` crate for field-level rules; `serde(deny_unknown_fields)` on every request DTO.
- Frontend: `zod` schemas for forms.
- **Validation logic lives in both places, owned separately.** We do not share schemas across Rust and TypeScript in v1. Backend is the source of truth; frontend duplicates for UX responsiveness. A regression test in the backend covers every rule.

## Response size

Keep responses small. Paginated list endpoints return at most 200 items. Chat message responses stream tokens rather than returning a single large body.

For endpoints that could return large payloads (transcripts, profile JSON), use pagination or streaming. Document when an endpoint may exceed 1 MB.

## Caching

- `GET /api/auth/me` returns `Cache-Control: no-store`.
- Static-ish admin data (`/api/admin/users`) returns `Cache-Control: no-cache, max-age=0` and relies on ETag if set.
- Avatars are served with `Cache-Control: private, max-age=3600`.
- Everything else: `Cache-Control: no-store`.

## CORS

Production: all API under the same origin as the frontend. CORS is **off** — same-origin by design.

Development: frontend `http://localhost:5173`, backend `http://localhost:8080`. Enable CORS with `Access-Control-Allow-Origin: http://localhost:5173`, `Access-Control-Allow-Credentials: true`. Behind a `DEV_CORS=1` env switch so it cannot be accidentally enabled in production.

## Streaming (SSE)

Used for chat token streaming and optional document ingestion events.

- Endpoint returns `text/event-stream`.
- Client must use `fetch` + `ReadableStream` to POST a body and consume the stream. The native `EventSource` API only supports GET; we do not build on it.
- Frames follow the SSE spec: `event: <name>\ndata: <json>\n\n`.
- Heartbeat comment `: keep-alive` every 15 s to survive proxy timeouts.
- Server caps stream duration to 10 minutes; client should retry on close if stream ended abnormally.

## Upload semantics

- `POST` with `multipart/form-data`.
- Required file field first so infer-based MIME detection can short-circuit.
- Other fields follow.
- Per-request body size limit enforced both at the reverse proxy (600 MB) and at axum (`DefaultBodyLimit`).
- Response is **synchronous for the metadata** (`201` with the created row), **asynchronous for the heavy work** (ingestion runs in the background; status transitions are observed via polling or SSE).
- Accepts `Idempotency-Key` header (see above).

## Testing

Every endpoint has:
- A happy-path test.
- A "another user cannot access" test (for user-scoped resources).
- A validation-failure test (at least one field-level violation).
- A 404 vs 403 test where relevant.

Documented in [`05-engineering-practices.md`](05-engineering-practices.md).
