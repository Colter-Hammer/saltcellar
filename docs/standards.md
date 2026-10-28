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

## Hygiene

| Rule | Check | Where |
| --- | --- | --- |
| UTF-8, LF line endings, final newline, no trailing whitespace, spaces for indentation. | `.editorconfig` | editor |
| One Node version for everyone. | `.nvmrc` | `nvm use` |
