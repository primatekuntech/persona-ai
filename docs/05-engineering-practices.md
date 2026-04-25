# Engineering practices

Opinionated standards for how this project is built. These apply to every sprint. The goal is a codebase that is easy to change in six months without fear, where tests are trusted, and where no single file is too surprising to read.

## Test-Driven Development (TDD)

### Default rhythm

**Red → Green → Refactor**, applied to units small enough to turn around in 2–5 minutes.

1. Write a failing test that expresses one behaviour.
2. Write the simplest code that makes it pass.
3. Refactor both test and code for clarity. Tests stay green.

### Where TDD is non-negotiable

- **Authorization boundaries.** Every repository function that scopes by `user_id` gets a test that asserts cross-user access is denied.
- **Parsers and analysers.** `chunker`, `lexical`, `syntactic`, `semantic`, `stylistic` — all pure functions, all ideal TDD targets.
- **Prompt builder.** The persona prompt assembler is a pure function from `(profile, era, exemplars, retrieved, user_msg)` to a string. Test output snapshots.
- **Error mapping.** `AppError → HTTP response`.

### Where TDD is flexible

- **UI components.** Write tests after the design stabilises; prefer integration tests (Playwright) over component unit tests.
- **Exploratory integrations.** First experiment with a crate, then extract a testable interface, then write tests.

### Test pyramid

```
        ┌──────────────┐
        │  E2E (few)   │  Playwright: smoke flows (login, invite, upload, chat)
        ├──────────────┤
        │ Integration  │  Axum TestServer + test Postgres container
        │  (moderate)  │  repositories, routes, worker jobs
        ├──────────────┤
        │    Unit      │  Pure functions: chunker, prompt builder, analysers
        │   (many)     │
        └──────────────┘
```

Target ratios: ~ 70 % unit, ~ 25 % integration, ~ 5 % E2E.

### Tooling

- **Rust:** `cargo test` with `#[tokio::test]` for async; `sqlx::test` (macro) for per-test database transactions that auto-rollback; `insta` for snapshot tests (prompt output, JSON profiles).
- **Frontend:** `vitest` for unit, `@testing-library/react` for component, `playwright` for E2E.
- **Coverage:** `cargo-llvm-cov` and `vitest --coverage`. Do not chase a number; treat coverage as a *finder* of untested code, not a KPI.

### Fixtures

- `backend/tests/fixtures/` holds small corpus samples for analyser tests (a 500-word text with known TTR, etc.).
- Golden files for prompt output live beside their tests as `.snap.md` (via `insta`).

### Writing good tests

- One behaviour per test. Test name describes the behaviour in plain English.
- Arrange, Act, Assert, with blank lines between.
- Do not assert on implementation details (e.g. don't test that a private helper was called).
- Prefer *example-based* tests for pure functions; consider `proptest` for anything with a clear invariant (chunker always preserves character content minus whitespace).

## SOLID

Applied pragmatically; SOLID is a smell detector, not a rule set.

### Single responsibility

Each module owns one concept. `services/embedder.rs` embeds, `services/chunker.rs` chunks, `services/retriever.rs` retrieves. No module combines two unless the combination is its purpose (`ingest` is allowed to orchestrate chunking + embedding).

A file > 400 lines is a signal to split. A function > 50 lines likewise.

### Open/closed

For features like style analysis, design so new analysers (new metrics) can be added without modifying the existing ones. The profile is a sum of contributions:

```rust
trait Analyser {
    fn name(&self) -> &'static str;
    fn compute(&self, chunks: &[Chunk]) -> serde_json::Value;
}

fn build_profile(chunks: &[Chunk], analysers: &[Box<dyn Analyser>]) -> Profile { ... }
```

New analyser → new struct → register it. No edits to existing analysers.

### Liskov

Trait impls must honour the trait's contract. A `Storage` impl that silently drops writes is not a `Storage`. Document invariants on traits (rustdoc).

### Interface segregation

Small, role-sized traits beat god-traits. Our `Storage` trait has `put`, `get`, `delete` because that's all the callers need. We do not bolt on `list_prefix` unless a caller actually requires it.

### Dependency inversion

Handlers depend on trait objects, not concrete types, where useful.

- `AppState { db: PgPool, email: Arc<dyn Emailer>, llm: Arc<dyn LlmRuntime>, ... }`.
- Swapping `Emailer` for a `FakeEmailer` in tests is trivial.
- Do not abstract prematurely: `sqlx::PgPool` stays concrete because we don't swap databases; abstracting it would be ceremony.

**Rule of thumb:** introduce a trait when there are ≥ 2 real implementations (prod + test counts as two) OR when the boundary is a known variation point.

## DRY — and its limit

Duplicated **logic** is a bug; duplicated **structure** is often fine. Three similar functions that evolve differently are better than one abstracted function with a flag argument.

### Apply DRY to

- SQL queries that encode the user-scoping invariant. Build them in one place (`repositories::chunks::for_user_persona_era`).
- Error → HTTP response mapping. Exactly one `IntoResponse` for `AppError`.
- Config parsing. One `AppConfig` loader.

### Resist DRY for

- Route handlers. A handler is allowed to be a linear, readable function. Extracting shared helpers is fine; extracting a handler framework is not.
- Tests. Repetition in tests aids readability; "shared setup helpers" that hide what's being tested do not.
- Small string formatting. Three `format!` calls are fine; a "string builder helper" is not.

**The WET rule (Write Everything Twice) beats DRY in the early life of a feature.** Wait for the third occurrence before extracting.

## Naming

- Crates, modules, functions: `snake_case`.
- Types, traits, enums: `UpperCamelCase`.
- Constants, env vars: `SCREAMING_SNAKE_CASE`.
- Never use abbreviations except widely accepted ones (`id`, `db`, `ctx`, `uuid`, `json`, `http`, `api`).
- Prefer verbs for functions that act, nouns for functions that return. `compute_profile` > `profile_computer`.
- Boolean functions start with `is_`, `has_`, `can_`. Boolean variables too.

## Commits & branches

- Trunk-based. Short-lived feature branches (< 2 days).
- Commit messages: imperative mood, < 72-char subject, body wrapped at 100.
- Conventional commits optional but consistent: `feat:`, `fix:`, `chore:`, `refactor:`, `test:`, `docs:`, `deps:`.
- One logical change per commit. Avoid "wip" in `main`.

## Code review (for later, even if solo)

Even when building alone, review your own PRs after sleeping on them. A diff read cold reveals what the writer's eye skipped.

Checklist:

- [ ] Does every change have a test that would fail without the code change?
- [ ] Does the code scope by `user_id` wherever it touches domain data?
- [ ] Any `.unwrap()` or `expect()` in non-test code? Justify or remove.
- [ ] Any hand-rolled SQL missing parameter binding? (sqlx forbids this by default — good.)
- [ ] Does it add a new config value? Is it documented in `.env.example`?
- [ ] Does it add a migration? Is the migration reversible in theory, and safe in practice?
- [ ] Does it add a public API? Is it referenced in a sprint doc or added to a sprint doc?

## Continuous integration

Single workflow file (`.github/workflows/ci.yml`) runs on every push and PR:

1. Lint: `cargo fmt --check`, `cargo clippy -D warnings`, `pnpm lint`.
2. Typecheck: `pnpm tsc --noEmit`.
3. Unit tests: `cargo test --lib`, `pnpm test:unit`.
4. Integration tests: spin up Postgres service container; `sqlx migrate run`; `cargo test --test '*'`.
5. Build: `cargo build --release` (sanity), `pnpm build` (sanity).
6. E2E (only on `main` pushes): Playwright against the built image.

Caching: `Swatinem/rust-cache` for Cargo, built-in `actions/setup-node` cache for pnpm.

## Static analysis

- `cargo clippy -D warnings` on every CI run. Treat warnings as failures.
- `cargo deny check` for licence and advisory scanning (run weekly at minimum).
- `cargo audit` for RUSTSEC advisories.
- Frontend: `eslint` strict config + `@typescript-eslint/strict`.
- Typescript strict mode on (`"strict": true` in tsconfig).

## Formatting

- Rust: `rustfmt` with defaults. One style battle skipped forever.
- Frontend: `prettier` + `eslint` via a pre-commit hook (`husky` + `lint-staged`).
- EditorConfig at repo root for universal settings (LF, 4-space Rust, 2-space TS, trim trailing whitespace).

## Error handling

- Libraries (`thiserror`) define domain errors.
- The `main` binary (`anyhow`) wraps and adds context.
- Never ignore `Result`. If you truly mean "fire and forget", write it explicitly: `let _ = ...;` with a comment.
- No `unwrap()` in production code paths. Tests may use it freely.
- Errors are logged at the highest layer that has full context, not re-logged at every layer.

## Logging

- Use `tracing::info!`, `debug!`, `warn!`, `error!` with structured fields (`tracing::info!(user_id = %id, "login")`).
- Do not log secrets, PII, or message content.
- `debug!` for developer-oriented facts; `info!` for operator-oriented facts; `warn!` for recoverable problems; `error!` for bugs or external failures.

## Documentation

- Every public Rust item has a doc comment. `#[deny(missing_docs)]` in library crates.
- Internal modules need doc comments only where non-obvious.
- The sprint docs in `docs/sprints/` are the source of truth for intent. When code diverges from a sprint doc, update the doc in the same PR.
- No separate "design docs" that duplicate sprint content.

## Dependency policy

- Add a dependency when it replaces ≥ 50 lines of non-trivial code OR encapsulates significant domain knowledge.
- Prefer battle-tested crates (`>= 1.0`, many downloads, recent releases, healthy issue tracker).
- Check licence — MIT/Apache-2.0 preferred; GPL/AGPL disallowed.
- Pin minor versions (`"1.5"`, not `"1"`) in Cargo.toml / package.json. Lockfiles are committed.
- `cargo outdated` run monthly; triage on merge.

## Secrets

- Never commit `.env` or any key material.
- `.env.example` stays in the repo with all variable names and safe defaults.
- Rotate secrets on compromise suspicion, not on a schedule (unless the org requires).

## Performance discipline

- Add a benchmark (`criterion`) before making a claim that something is "too slow" or "fast enough".
- Profile before optimising. `tokio-console` for async; `flamegraph` for CPU; `heaptrack` for allocations.
- Fast code that's hard to read is worse than slow code that's easy to read, unless the slowness is user-visible.

## Accessibility & internationalisation

- Keyboard accessibility is a correctness concern, not a nice-to-have. Every interactive element reachable by tab, operable by enter/space.
- Colour contrast meets WCAG AA minimum, AAA where possible (already baked into the design tokens).
- Copy is English-only in v1; lift all user-facing strings into a single module so future i18n is mechanical.
- `axe-core` runs in CI via `@axe-core/playwright`: end-to-end tests assert zero violations on every primary page (login, personas, dashboard, upload, chat, admin). Violations fail the build.
- Visual regression is **not** in CI v1. Accessibility is; visual diffing is lower value.

## Validation parity

Backend (`validator` + `serde(deny_unknown_fields)`) is the source of truth for every input rule. Frontend (`zod`) duplicates the rules for immediate UX feedback. The two are **not** shared — no Rust-to-TS type generation, no OpenAPI emission, no JSON Schema mirror in v1.

Consequences:
- A backend validation change requires a matching frontend change in the same PR. Checklist item in the PR template.
- Integration tests cover every backend rule. If frontend misses a rule, backend still rejects — client-side validation is UX, server-side is correctness.
- When drift is observed in production (client lets through, server rejects), the bug is fixed in both places in one commit.

If drift becomes a recurring problem, we'll revisit shared schemas in v2; forcing shared types now couples Rust and TS unnecessarily.

## The spirit

- Clarity beats cleverness.
- Deleting code is a feature.
- Boring technology where it doesn't matter; careful, explicit code where it does (auth, data model, persona prompt).
- When in doubt, write the test first.
