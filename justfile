# `just check` runs exactly what CI runs. See CONTRIBUTING.md and docs/standards.md.

set shell := ["bash", "-euo", "pipefail", "-c"]

# List the recipes.
default:
    @just --list

# Install the git hooks (pre-commit, commit-msg, pre-push) and the frontend's dependencies.
setup:
    lefthook install
    npm --prefix web ci

# The API and the frontend together: open http://localhost:5173 (Vite, proxying /api to the API at BUDGET_LISTEN).
dev:
    #!/usr/bin/env bash
    set -euo pipefail
    trap 'kill 0' EXIT
    just dev-api &
    npm --prefix web run dev

# Run the server on http://localhost:8080, with its data in ./data (git-ignored).
dev-api:
    BUDGET_DATA_DIR="${BUDGET_DATA_DIR:-data}" BUDGET_LISTEN="${BUDGET_LISTEN:-127.0.0.1:8080}" cargo run --locked --bin budget

# Every check CI runs.
check: leak-guard file-rule gitleaks fmt clippy deny layering test coverage sqlx-offline migrations api-contract web-typecheck web-lint web-format web-knip web-test web-coverage web-build

# Denylisted personal terms anywhere in the history.
leak-guard:
    python3 scripts/guard.py leak history

# Denied file types outside fixtures/ and files over 1 MiB anywhere in the history.
file-rule:
    python3 scripts/guard.py files history

# Secrets anywhere in the history.
gitleaks:
    gitleaks git --redact --no-banner

# Rust formatting.
fmt:
    cargo fmt --all --check

# Clippy with the workspace lints, warnings denied.
clippy:
    cargo clippy --workspace --all-targets --locked -- -D warnings

# Advisories, licences, bans and duplicates, sources.
deny:
    cargo deny --locked check

# `domain` doesn't depend on axum, sqlx or tokio.
layering:
    python3 scripts/layering.py

# Every Rust test.
test:
    cargo nextest run --workspace --locked

# Line coverage of `domain` at or above its floor in coverage-floors.txt, and no floor lowered.
coverage:
    python3 scripts/coverage.py rust

# The checked-in sqlx query data (.sqlx/) matches the queries and the migrations.
sqlx-offline:
    scripts/sqlx-offline.sh check

# Regenerate .sqlx/ after adding or changing a query.
sqlx-prepare:
    scripts/sqlx-offline.sh prepare

# Migrations that exist on main are unchanged; new ones are numbered after them.
migrations:
    python3 scripts/migrations.py

# The committed OpenAPI document and the frontend's generated types match the Rust API.
api-contract:
    scripts/gen-api.sh check

# Regenerate web/src/api/openapi.json and web/src/api/generated/ after changing a route or an API type.
gen-api:
    scripts/gen-api.sh write

# Frontend types, with vue-tsc.
web-typecheck:
    npm --prefix web run typecheck

# ESLint with the frontend rules and the import boundaries, warnings denied.
web-lint:
    npm --prefix web run lint

# Frontend formatting (Prettier).
web-format:
    npm --prefix web run format:check

# No unused files, exports or dependencies in the frontend.
web-knip:
    npm --prefix web run knip

# Every Vitest test.
web-test:
    npm --prefix web run test

# Line coverage of the frontend's logic at or above its floor in coverage-floors.txt, and no floor lowered.
web-coverage:
    python3 scripts/coverage.py web

# The frontend builds to web/dist.
web-build:
    npm --prefix web run build
