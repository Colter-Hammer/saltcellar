# Foundations

Everything every milestone builds on. Terms are from [/GLOSSARY.md](../../GLOSSARY.md); *(agent)* marks details placed by the agent rather than agreed with the owner.

## Stack

| Layer | Choice | Why / record |
|---|---|---|
| Backend | Rust, axum, sqlx (compile-time-checked queries) | A strict compiler catches LLM-written mistakes before runtime. [ADR-0001](../adr/0001-rust-backend.md) |
| Database | SQLite in WAL mode, one file in the data dir, owned by the app alone | [ADR-0002](../adr/0002-self-contained-sqlite.md) |
| Frontend | Vue 3 + TypeScript + Vite, built to static files served by the axum server | [ADR-0004](../adr/0004-vue-frontend.md) |
| Components | shadcn-vue (Reka UI) + Tailwind, copied into the repo and owned; TanStack Table for tables | [ADR-0004](../adr/0004-vue-frontend.md) |
| Charts | Apache ECharts via vue-echarts, one theme file reading the design tokens | |
| API contract | OpenAPI generated from Rust with `utoipa`; TypeScript client types generated from it | [ADR-0007](../adr/0007-generated-api-contract.md) |
| Server state in the browser | TanStack Vue Query *(agent)* | |
| Packaging | One Docker image, run with Docker Compose | |
| Push notifications | ntfy, behind a small notifier seam (milestone 6) | |
| Bank data | SimpleFIN Bridge; CSV/QFX files; manual entry (milestone 2) | ADR added with the milestone 2 sections |

Tool and crate versions are pinned during milestone 1 setup.

## Repository layout *(agent)*

```
Cargo.toml                 workspace, shared [lints]
rust-toolchain.toml
crates/
  domain/                  pure logic: money, dates and budget windows, ledger invariants, budget math,
                           categorizer, transfer matching, Actual mapping rules. No axum, no sqlx, no I/O.
  db/                      sqlx: migrations, queries, repositories returning domain types
  actual/                  reads an Actual export zip (read-only SQLite) into domain import types
  api/                     axum routes, auth, request/response types with utoipa schemas
  server/                  the binary: config, startup, background jobs, CLI subcommands, static files
web/                       Vue app (src/views → src/components → src/api), generated API types in src/api/generated
migrations/                sqlx migrations, append-only
fixtures/                  generated fake data only (incl. a fake Actual export builder)
scripts/                   leak guard, OpenAPI generation, migration-immutability check
docs/                      spec/, adr/, standards.md, deploy.md
Dockerfile, compose.yaml, justfile, lefthook.yml, .editorconfig
```

The layering is enforced by the compiler: `domain` has no dependency on `axum`, `sqlx` or `tokio`; `db` and `actual` depend on `domain`; `api` depends on `domain` and `db`; `server` wires everything. A CI check fails if `domain`'s dependency tree gains any of those crates.

## Conventions

- **Money** is integer minor units of the household currency (`i64`), wrapped in a `Money` newtype in `domain`. Never floats. Display uses the currency's decimal places and the viewer's locale (`Intl.NumberFormat`). One currency per install, chosen at setup, fixed once any transaction exists ([ADR-0006](../adr/0006-one-currency-one-timezone.md)).
- **Sign** is from the account's point of view: money leaving is negative. Spending totals are shown as positive numbers; a refund is negative spending in its original category.
- **Dates** are plain calendar dates (`YYYY-MM-DD`) with no timezone. Months are `YYYY-MM`. Timestamps are UTC.
- **Household timezone** (IANA) defines "today", budget window boundaries, and when scheduled jobs run. Pre-filled from the browser at setup, editable in Settings.
- **Locale** is per login (default: browser locale), used only for date and number formats; week start comes from it, with a per-login override. Stored data never depends on it.
- **English only**, no i18n framework.
- **Ids** are SQLite integer primary keys; never exposed as anything but opaque numbers in the API.
- **Invariants** marked "enforce in code" in [schema.sql](schema.sql) live in `domain` with unit tests.

## Configuration

Environment variables cover only what's needed before anyone can log in, or what touches the host. Everything else lives in the in-app **Settings**, stored in the database (the owner prefers changing things in a settings page). Env names are *(agent)*; the set is agreed.

| Variable | Default | Meaning |
|---|---|---|
| `BUDGET_DATA_DIR` | `/data` | Holds the SQLite file and, by default, backups. |
| `BUDGET_LISTEN` | `0.0.0.0:8080` | Listen address. |
| `BUDGET_BASE_URL` | none (required from milestone 6) | Public URL of the app, used in invite, reset and ntfy links. |
| `BUDGET_AUTH_MODE` | `builtin` | `builtin` or `header`. |
| `BUDGET_TRUSTED_PROXY` | none | Header mode: the proxy address (IP or CIDR) whose header is trusted. Required in header mode. |
| `BUDGET_AUTH_HEADER` | `Remote-User` | Header mode: the header carrying the username. |
| `BUDGET_BACKUP_DIR` | `$BUDGET_DATA_DIR/backups` | Where nightly backups are written. |

Bank sync adds its own (e.g. the key that encrypts the connection secret) in the milestone 2 sections. Startup fails with a clear message if a required variable is missing or invalid.

## HTTP API *(agent)*

- JSON under `/api/v1/…`; the OpenAPI document at `/api/openapi.json`. Everything else serves the built frontend, with unknown paths falling back to `index.html`.
- Errors return `{ "error": { "code": "snake_case_code", "message": "Human sentence." } }` with a fitting status. Validation errors add `"fields": { name: message }`.
- Mutating requests must be JSON (`Content-Type: application/json`), which with `SameSite=Lax` cookies blocks cross-site form posts. No CORS.
- Lists the UI pages through (transactions) use cursor pagination on `(date, id)`.
- `GET /api/v1/health` (no auth) for the Docker healthcheck: 200 when the database answers.

## Auth

Two modes, one active per install, set by env ([ADR-0005](../adr/0005-two-auth-modes.md)).

**Built-in login (default, milestone 1).**
- Passwords: at least 12 characters, no composition rules, hashed with argon2id.
- Sessions: server-side rows, 30-day sliding expiry, cookie `HttpOnly` + `Secure` + `SameSite=Lax` (the `Secure` flag is skipped only when the request is to `localhost`, so local dev works over plain HTTP *(agent)*). "Log out everywhere" deletes all of a login's sessions.
- Throttle: 5 failed attempts in 15 minutes pause that username for 15 minutes. Fixed in code, not configurable.
- Recovery: `budget reset-password <login>` (run with `docker compose exec app …`) prints a one-time reset link (milestone 1). From milestone 6, any other login can also issue one from Settings. No email.

**Trusted-header mode (opt-in, milestone 6).** The app trusts `BUDGET_AUTH_HEADER` only on requests whose source address matches `BUDGET_TRUSTED_PROXY`; anything else is rejected. The header value maps to a login's `remote_user`; an unknown value is rejected. The app stores no passwords in this mode. Intended for a forward-auth proxy (any SSO that sets a header).

**Claiming a fresh install (both modes).** On startup, while setup isn't finished, the app prints a one-time setup code to the container log. The setup wizard asks for it first. It stops working once setup finishes. A new code is printed on each start until then *(agent)*.

**Logins.** All equal: no admin role, no cap on the count. Any login can remove another; the last login can't remove itself. Invites arrive in milestone 6.

## Background jobs *(agent)*

One in-process scheduler in `server`, ticking every minute, running jobs at times in the household timezone. Each job is single-flight (a second trigger joins the running one) and records its outcome. Jobs by milestone: bank sync and its retries (M2), classifier retraining nightly (M3), nightly backup and the restore-drill reminder (M6), review reminders and nudges (M6). A missed nightly job runs a few minutes after startup.

## Logging

Structured logs via `tracing`, one line per request and per job. Never log amounts, merchants, descriptions, tokens, passwords or the bank access URL. The setup code and CLI reset link are the only secrets ever printed, and only on purpose.

## Frontend

- **Layout: mobile-first.** Phone: header + bottom tab bar. Desktop (from the `lg` breakpoint *(agent)*): sidebar, with extra columns and side panels on dense views.
- **Navigation (full v1).** Phone tab bar: Overview · Transactions · Review (count badge) · Budget · More. Desktop sidebar: the same top items, then a **Reports** section (Spending, Trends, Cash flow) and a **Money** section (Funds, Savings goals, Transfers), with Accounts and Settings, and the sync status spot at the foot (milestone 2).
- **Navigation grows with the milestones** *(agent)*: only screens that exist are shown. Milestone 1: Transactions · Accounts · More (Categories, Import from Actual, Settings), landing on Transactions. Overview becomes the landing screen in milestone 4.
- **Design tokens** (color, spacing, radius, typography) live in one place and feed both Tailwind and the ECharts theme. Layouts are grayscale until the styling pass.
- **Formatting** goes through one module: money with the household currency and the login's locale, dates with the login's locale and week start.
- **Import boundaries:** views → components → API client. Views never call `fetch`; components never import views. Enforced by lint.
- **PWA** (milestone 6): installable; last-loaded data cached read-only; no offline editing. Needs HTTPS from the server.

## Coding standards

Set up as the first work of milestone 1, before any feature code. `docs/standards.md` lists every rule beside the check that enforces it; `CONTRIBUTING.md` explains how to run them; `CLAUDE.md` only points at these and adds no rules of its own.

**Merge gate.** Private GitHub repo; `main` protected; changes land by pull request only; every CI check required; no bypass for anyone. Local `lefthook` hooks run the fast subset; CI runs everything. `just check` runs exactly what CI runs.

**Nothing personal in the repo** ([ADR-0008](../adr/0008-nothing-personal-in-the-repo.md)).
- Default-deny ignores: `.env*`, `data/`, backups, `*.sqlite`, `*.zip`, `*.age`, and `*.csv` / `*.ofx` / `*.qfx` / `*.qbo` / `*.json` data dumps are never committable, except under `fixtures/` (generated fake data only) and the tool config files that must be JSON (`package.json`, `tsconfig*.json`, and the like, listed explicitly). `.scratch/` is ignored.
- Pre-commit and pre-push hooks run `gitleaks`, the **leak guard** (a denylist of personal names, account suffixes, IPs, domains and payees) and a file-type/size rule, so nothing bad leaves the PC. CI repeats all of it over the full history.
- The denylist itself is personal, so it lives outside the repo: a local file for the hooks and a GitHub Actions secret for CI. The repo holds only the script that reads it. The leak guard has no excluded paths.
- Project Claude settings deny `--no-verify`. Turn on GitHub secret scanning / push protection if the plan offers it for private repos.

**Rust.** `rustfmt` checked; workspace `[lints]` with clippy `pedantic` denied and `-D warnings`; `unsafe_code = "forbid"`; `unwrap_used`, `expect_used`, `panic`, `todo`, `dbg_macro`, `print_stdout` denied outside tests; `cargo-deny` (advisories, licences, bans, duplicates); sqlx offline query data checked in CI; toolchain pinned in `rust-toolchain.toml`.

**Frontend.** TypeScript `strict` + `noUncheckedIndexedAccess` + `exactOptionalPropertyTypes`; `vue-tsc --noEmit`; ESLint flat config with `typescript-eslint` strict-type-checked + `eslint-plugin-vue`, zero warnings; Prettier checked; no `any`, no non-null `!`; import boundaries linted; `knip` for dead code and unused dependencies.

**API contract.** CI regenerates the OpenAPI document and the TypeScript types and fails if either differs from what's committed. No hand-written request or response types in the frontend.

**Tests.** `cargo nextest` and Vitest. Coverage floors on `domain` and on frontend logic (`cargo-llvm-cov`, Vitest coverage) that only ratchet up. One Playwright smoke test per milestone's main flow, on fake data only.

**Hygiene.** `.editorconfig`; Conventional Commit PR titles checked; Renovate for dependency updates; shipped migrations can't be edited (CI compares them with `main`).

## Test data

All test data is generated and fake: a made-up household, invented accounts, payees and amounts. A fixture builder in `fixtures/` produces a fake Actual export zip (a SQLite file with the subset of Actual's schema the importer reads, see [milestone 1](m1-ledger-import.md#reading-the-export)) covering every case the importer handles: tombstones, merged payees and categories, a split, a linked transfer pair, starting-balance rows, closed accounts, notes with `#tags`, uncategorized rows. The owner's real export is only ever used locally, never committed.

## Deployment

The repo ships a multi-stage `Dockerfile` (builds the frontend and the Rust binary; small runtime image running as a non-root user, with a healthcheck on `/api/v1/health`) and an example `compose.yaml` that builds from source, with the image swappable through a variable. `docs/deploy.md` (written in milestone 1, extended at launch) has the env table, says to run behind any HTTPS reverse proxy (needed for the PWA and `Secure` cookies), and gives one minimal Caddy example with a placeholder domain. Any specific home-server setup lives outside this repo.
