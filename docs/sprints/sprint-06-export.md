# Sprint 6 — Export: generate Markdown and .docx

**Goal:** the user can export any assistant message, or a stitched selection across a session, to `.md` or `.docx`. File downloads feel instant.

This is session-scoped export (chat → `.md`/`.docx`). Full-account export (entire corpus + profiles, produced as a `.zip`) lives in [`sprint-07-polish-deploy.md`](sprint-07-polish-deploy.md#722-full-account-export) and drives the self-service data-rights deliverable.

**Duration estimate:** 2–3 days.

## Deliverables

1. Endpoints to export a message or a range of messages.
2. Markdown export (straight passthrough of stored content).
3. `.docx` export via `docx-rs`, with a clean, minimalist layout.
4. Frontend: export menu on assistant messages + bulk export on session.

## Backend tasks

### 6.1 Endpoints

```
GET /api/chats/:session_id/export
    ?format=md|docx
    &message_ids=<comma-separated>     optional; if absent, exports all assistant messages in the session
    &title=<optional>

Response: file stream with
  Content-Disposition: attachment; filename="<sanitised>.{md,docx}"
  Content-Type: text/markdown | application/vnd.openxmlformats-officedocument.wordprocessingml.document
```

Ownership: session must belong to the user; message ids must belong to the session.

### 6.2 Markdown export

Trivial — for each message, write:

```
# <title or "<Persona> — <session.title or date>">

_<persona.name>, {era.label}_

---

<content>

---

_Generated from <n> source(s) in your own corpus._
```

Stream with `Content-Type: text/markdown; charset=utf-8`.

### 6.3 .docx export

Use `docx-rs`. Keep layout minimal:

- Default font: Calibri 11 (Word's default; avoids the "document imported from web" look).
- Title: 18pt bold, centre.
- Subtitle (persona + era): 11pt italic, centre, grey.
- Body: 11pt, 1.2 line height, 1 cm paragraph spacing.
- Optional metadata footer on last page: generation date, persona name, era label, chat session id. All four are already on the rows we need (`personas.name`, `eras.label`, `chat_sessions.id`, `now()`); no new lookups. Do **not** include style-profile internals from [`sprint-04-analysis.md`](sprint-04-analysis.md#profile-schema) — the profile is not user-facing.
- Page: A4 with 2.54 cm margins.

Structure:

```rust
Docx::new()
  .add_paragraph(title_para())
  .add_paragraph(subtitle_para(persona, era))
  .add_paragraph(horizontal_rule())
  .add_paragraphs(body_paragraphs(content))       // split on \n\n
  .add_paragraph(footer_para(metadata))
  .build();
```

Render Markdown → plain paragraphs: strip headings, render lists as indented paragraphs, bold/italic mapped to runs, code blocks as monospace. We explicitly do *not* preserve heavy formatting — the output should look like a draft in Word, not a rendered webpage.

### 6.4 Filename

```
<persona-slug>_<era-slug-or-all>_<yyyymmdd-hhmm>.<ext>
```

Slug = lowercased, ASCII, non-alnum → `-`, collapsed.

### 6.5 Saved exports (optional)

Write the file to `/data/exports/<user_id>/<session_id>-<ts>.docx` and also return it. Keeps an audit trail; lets the user re-download without regenerating. A cron (sprint 7) prunes files older than 30 days.

## Frontend tasks

### 6.6 Per-message export

Dropdown on assistant messages: "Copy", "Export as .md", "Export as .docx". Uses window.location to trigger download.

### 6.7 Session export

Button in chat header: "Export session…" opens a dialog:
- Title (default: first 60 chars of first assistant message).
- Checkboxes of assistant messages (all selected by default).
- Format toggle (md / docx).
- "Export" triggers download.

## Acceptance tests

1. Exporting a single message as `.docx` yields a 10–30 KB file that opens in Word and LibreOffice without warnings.
2. Filename contains persona slug and date.
3. Markdown export round-trips: downloading as `.md` then re-rendering in a markdown viewer matches the chat output.
4. Attempting to export a session that doesn't belong to the user → 404.
5. Exporting an empty selection → 400 with a clear message.

## Out of scope

- PDF export.
- Custom templates.
- Emailing the export.
- Batch exporting multiple sessions at once.
