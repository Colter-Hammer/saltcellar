# Standards

Every rule in the repo beside the check that enforces it. The spec's [coding standards](spec/foundations.md#coding-standards) say what the rules are; this table says how each one is checked and where. A change that adds a check adds its row here. How to run them: [CONTRIBUTING.md](../CONTRIBUTING.md).

Where: **hook** = local `lefthook` hook (`pre-commit`, `commit-msg`, `pre-push`); **check** = `just check`, which is what CI runs; **CI** = a job in `.github/workflows/`, each a required check on `main` except `e2e`, which the pre-push hook gates instead.

## Merge gate

| Rule | Check | Where |
| --- | --- | --- |
| Changes reach `main` by pull request only: no direct pushes, no force-pushes, no deletion, no bypass for admins. | GitHub branch protection on `main` | GitHub |
| Every CI job passes on a branch that is up to date with `main`. | Branch protection requires each job in `ci.yml` and `pr-title.yml` | GitHub |
| `just check` runs exactly what CI runs. | CI job `just-check` runs the recipe beside the per-check jobs | CI |
| PR titles are Conventional Commits; PRs are squash-merged with the title as the commit subject. | CI job `pr-title` (`.github/workflows/pr-title.yml`); squash is the only merge method | CI, GitHub |
| Dependencies stay current: grouped weekly updates, lockfile maintenance on, actions pinned by digest. | Renovate (`renovate.json`) | GitHub |

## Nothing personal in the repo

[ADR-0008](adr/0008-nothing-personal-in-the-repo.md).

| Rule | Check | Where |
| --- | --- | --- |
| Personal data files are never committed: `.env*`, `data/`, backups, `*.sqlite*`, `*.zip`, `*.age`, and `*.csv` / `*.ofx` / `*.qfx` / `*.qbo` / `*.json` outside `fixtures/` (tool JSON configs and the generated `web/src/api/openapi.json` listed one by one, `web/components.json` among them). `.scratch/` stays local. | `.gitignore` default-deny | `git add` |
| The same file types can't be committed even if forced past `.gitignore`; no file over 1 MiB. | File rule: `scripts/guard.py files` | hook: pre-commit (staged), pre-push (the commits being pushed); check, CI `file-rule`: full history |
| No personal terms (names, account suffixes, banks, places, domains, IPs, payees) in any file, file name or commit message. No excluded paths. The denylist lives outside the repo. | Leak guard: `scripts/guard.py leak` | hook: pre-commit (staged), commit-msg, pre-push (the commits being pushed); check, CI `leak-guard`: full history (denylist from the `LEAK_DENYLIST` Actions secret) |
| The leak guard never passes without a denylist. | `scripts/guard.py` exits with an error naming the expected path | everywhere the leak guard runs |
| No secrets (tokens, keys, credentials). | `gitleaks` with `.gitleaks.toml` (default rules) | hook: pre-commit (staged), pre-push (the commits being pushed); check, CI `gitleaks`: full history |
| Hooks aren't skipped by agents. | `.claude/settings.json` denies `--no-verify`, `-n`, `core.hooksPath` overrides and `LEFTHOOK=0` | Claude sessions |

## Rust

| Rule | Check | Where |
| --- | --- | --- |
| One toolchain for everyone, with `clippy`, `rustfmt` and `llvm-tools-preview`. | `rust-toolchain.toml` (pinned by hand; Renovate doesn't bump it) | `cargo`, CI |
| Code is `rustfmt`-formatted. | `cargo fmt --all --check` | check, CI `fmt` |
| Clippy `pedantic` is denied, and so is every warning. `unwrap`, `expect`, `panic!`, `todo!`, `dbg!` and `println!` are denied outside tests. `unsafe` is forbidden. Every crate takes the workspace lints. | `[workspace.lints]` in `Cargo.toml`, `clippy.toml` (the in-tests allowances), `cargo clippy --workspace --all-targets -- -D warnings` | check, CI `clippy` |
| No dependency with a RustSec advisory (unmaintained and unsound included) or yanked; only the licences in `deny.toml`; one version of each crate (each exception listed with its reason); crates.io only. | `cargo deny check` with `deny.toml` | check, CI `deny` |
| `domain` is pure logic: its dependency tree has no `axum`, `sqlx` or `tokio` crate. | `scripts/layering.py` | check, CI `layering` |
| Builds never need a database: the sqlx query data in `.sqlx/` is checked in and matches every query and the migrations. Regenerate it with `just sqlx-prepare`. | `scripts/sqlx-offline.sh check` (`cargo sqlx prepare --check` against a fresh database); `SQLX_OFFLINE` in `.cargo/config.toml` | check, CI `sqlx-offline` |
| Migrations are append-only: one that exists on `main` is never edited or deleted. New ones are named `NNNN_snake_case.sql` and numbered after `main`'s newest. | `scripts/migrations.py` (compares `migrations/` with `main`); sqlx also refuses to start on a changed applied migration | check, CI `migrations`; startup |

## Frontend

The frontend is `web/`. Its checks run with `npm --prefix web run <script>`; `just web-<check>` wraps each.

| Rule | Check | Where |
| --- | --- | --- |
| TypeScript is `strict`, with `noUncheckedIndexedAccess` and `exactOptionalPropertyTypes`, in `.ts` and `.vue` files alike. | `vue-tsc -b` with `web/tsconfig.app.json` and `web/tsconfig.node.json` | check, CI `web-typecheck` |
| ESLint `typescript-eslint` strict-type-checked and stylistic-type-checked, plus `eslint-plugin-vue` recommended; zero warnings. That includes no `any` and no non-null `!`. | `eslint --max-warnings 0` with `web/eslint.config.ts` | check, CI `web-lint` |
| Import boundaries: views → components → API client. Only `src/api/` calls `fetch` (bare, or on `window`, `globalThis` or `self`); components never import views; `src/api/` and `src/lib/` import neither. | `no-restricted-globals`, `no-restricted-properties` and `no-restricted-imports` in `web/eslint.config.ts`; `web/checks/lint.test.ts` proves each rule fires | check, CI `web-lint`, `web-test` |
| API types are generated, never hand-written ([ADR-0007](adr/0007-generated-api-contract.md)): the committed `web/src/api/openapi.json` and `web/src/api/generated/` match what the Rust API generates. Regenerate them with `just gen-api`. | `scripts/gen-api.sh check` (the `api` crate's `openapi` example, then `openapi-typescript`, diffed with what's committed) | check, CI `api-contract` |
| Only `src/api/` uses the generated types and the typed client (`openapi-fetch`); the rest of the app imports `src/api/`'s functions and hooks. Hand-written request and response types are caught in review. | `no-restricted-imports` in `web/eslint.config.ts`; `web/checks/lint.test.ts` proves it fires | check, CI `web-lint`, `web-test` |
| Code is Prettier-formatted. | `prettier --check` (config in `web/package.json`) | check, CI `web-format` |
| No unused files, exports or dependencies. | `knip` | check, CI `web-knip` |
| The frontend builds. | `vite build` into `web/dist` | check, CI `web-build` |
| One Node version for everyone; dependencies installed exactly from the lockfile. | `.nvmrc`; `npm ci` in `.github/actions/web` | CI |

## Tests

| Rule | Check | Where |
| --- | --- | --- |
| Every Rust test passes. | `cargo nextest run --workspace` | check, CI `test` |
| The fake Actual exports committed in `fixtures/` match what their builder (`crates/actual/src/fake.rs`) writes, compared by content (zip entries, tables and rows), so a new zlib or SQLite doesn't count as drift. Regenerate them with `just fixtures`. | the `actual` crate's test `the_committed_fixtures_match_the_builder` | check, CI `test` |
| Every Vitest test passes. | `vitest run` | check, CI `web-test` |
| Line coverage of `domain` is at or above its floor, and floors only go up: lowering or removing one fails. | `scripts/coverage.py rust` with `coverage-floors.txt` (`cargo llvm-cov nextest`) | check, CI `coverage` |
| Line coverage of the frontend's logic (the TypeScript modules in `web/src/`, tests and `main.ts` aside) is at or above the `web` floor, which only goes up the same way. | `scripts/coverage.py web` with `coverage-floors.txt` (Vitest coverage, v8) | check, CI `web-coverage` |
| The app image builds, runs as the non-root user and reports healthy; Playwright's tests pass against it on a fresh data dir, in a phone and a desktop layout, after a setup test claims the install with the code from its log. Fake data only. | `Dockerfile`, `web/playwright.config.ts`, `web/e2e/`; `just e2e` (compose) | hook: pre-push (when the checked-out commit is pushed; the working tree must be clean); check, CI `e2e` (`docker run`, image layers in the Actions cache; runs on every PR but isn't required) |

## Hygiene

| Rule | Check | Where |
| --- | --- | --- |
| UTF-8, LF line endings, final newline, no trailing whitespace, spaces for indentation. | `.editorconfig` | editor |
