# Standards

Every rule in the repo beside the check that enforces it. The spec's [coding standards](spec/foundations.md#coding-standards) say what the rules are; this table says how each one is checked and where. A change that adds a check adds its row here. How to run them: [CONTRIBUTING.md](../CONTRIBUTING.md).

Where: **hook** = local `lefthook` hook (`pre-commit`, `commit-msg`, `pre-push`); **check** = `just check`, which is what CI runs.

## Nothing personal in the repo

[ADR-0008](adr/0008-nothing-personal-in-the-repo.md).

| Rule | Check | Where |
| --- | --- | --- |
| Personal data files are never committed: `.env*`, `data/`, backups, `*.sqlite*`, `*.zip`, `*.age`, and `*.csv` / `*.ofx` / `*.qfx` / `*.qbo` / `*.json` outside `fixtures/` (tool JSON configs listed one by one). `.scratch/` stays local. | `.gitignore` default-deny | `git add` |
| The same file types can't be committed even if forced past `.gitignore`; no file over 1 MiB. | File rule: `scripts/guard.py files` | hook: pre-commit (staged), pre-push (the commits being pushed); check: full history |
| No personal terms (names, account suffixes, banks, places, domains, IPs, payees) in any file, file name or commit message. No excluded paths. The denylist lives outside the repo. | Leak guard: `scripts/guard.py leak` | hook: pre-commit (staged), commit-msg, pre-push (the commits being pushed); check: full history |
| The leak guard never passes without a denylist. | `scripts/guard.py` exits with an error naming the expected path | everywhere the leak guard runs |
| No secrets (tokens, keys, credentials). | `gitleaks` with `.gitleaks.toml` (default rules) | hook: pre-commit (staged), pre-push (the commits being pushed); check: full history |
| Hooks aren't skipped by agents. | `.claude/settings.json` denies `--no-verify`, `-n`, `core.hooksPath` overrides and `LEFTHOOK=0` | Claude sessions |

## Hygiene

| Rule | Check | Where |
| --- | --- | --- |
| UTF-8, LF line endings, final newline, no trailing whitespace, spaces for indentation. | `.editorconfig` | editor |
| One Node version for everyone. | `.nvmrc` | `nvm use` |
