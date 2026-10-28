# Standards

Every rule in the repo beside the check that enforces it. The spec's [coding standards](spec/foundations.md#coding-standards) say what the rules are; this table says how each one is checked and where. A change that adds a check adds its row here. How to run them: [CONTRIBUTING.md](../CONTRIBUTING.md).

Where: **hook** = local `lefthook` hook (`pre-commit`, `commit-msg`, `pre-push`); **check** = `just check`, which is what CI runs; **CI** = a job in `.github/workflows/`, each a required check on `main`.

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
| Personal data files are never committed: `.env*`, `data/`, backups, `*.sqlite*`, `*.zip`, `*.age`, and `*.csv` / `*.ofx` / `*.qfx` / `*.qbo` / `*.json` outside `fixtures/` (tool JSON configs listed one by one). `.scratch/` stays local. | `.gitignore` default-deny | `git add` |
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

## Tests

| Rule | Check | Where |
| --- | --- | --- |
| Every Rust test passes. | `cargo nextest run --workspace` | check, CI `test` |
| Line coverage of `domain` is at or above its floor, and floors only go up: lowering or removing one fails. | `scripts/coverage.py` with `coverage-floors.txt` (`cargo llvm-cov nextest`) | check, CI `coverage` |

## Hygiene

| Rule | Check | Where |
| --- | --- | --- |
| UTF-8, LF line endings, final newline, no trailing whitespace, spaces for indentation. | `.editorconfig` | editor |
| One Node version for everyone. | `.nvmrc` | `nvm use` |
