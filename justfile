# `just check` runs exactly what CI runs. See CONTRIBUTING.md and docs/standards.md.

set shell := ["bash", "-euo", "pipefail", "-c"]

# List the recipes.
default:
    @just --list

# Install the git hooks (pre-commit, commit-msg, pre-push).
setup:
    lefthook install

# Every check CI runs.
check: leak-guard file-rule gitleaks

# Denylisted personal terms anywhere in the history.
leak-guard:
    python3 scripts/guard.py leak history

# Denied file types outside fixtures/ and files over 1 MiB anywhere in the history.
file-rule:
    python3 scripts/guard.py files history

# Secrets anywhere in the history.
gitleaks:
    gitleaks git --redact --no-banner
