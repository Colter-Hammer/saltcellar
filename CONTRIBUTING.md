# Contributing

The rules are in [docs/standards.md](docs/standards.md), each beside the check that enforces it. This file is how to run those checks.

## Tools

Installed per user (no `sudo`), on `PATH` from `~/.local/bin` and `~/.cargo/bin`.

| Tool | Version | Install |
| --- | --- | --- |
| `rustup` | 1.29.2 | `curl https://sh.rustup.rs -sSf \| sh -s -- -y --profile minimal` |
| `just` | 1.59.0 | release binary from [casey/just](https://github.com/casey/just/releases) into `~/.local/bin` |
| `lefthook` | 2.2.2 | release binary from [evilmartians/lefthook](https://github.com/evilmartians/lefthook/releases) into `~/.local/bin` |
| `gitleaks` | 8.30.1 | release binary from [gitleaks/gitleaks](https://github.com/gitleaks/gitleaks/releases) into `~/.local/bin` |
| `gh` | 2.103.3 | release binary from [cli/cli](https://github.com/cli/cli/releases) into `~/.local/bin` |
| Node | see `.nvmrc` | `nvm install` in the repo root |
| Python | 3.10 or newer | the system `python3` (the guards use the standard library only) |

## First time

1. Put the leak-guard denylist at `~/.config/budget-leak-denylist.txt`, or set `BUDGET_LEAK_DENYLIST` to its path. It is personal, so it is never in the repo; ask the owner for it. The hooks and `just check` fail until it exists.
2. `just setup` installs the git hooks.

## Every change

- `just check` runs exactly what CI runs. Run it before opening a PR.
- The hooks run the fast subset on every commit and push. Don't skip them: CI runs the same checks over the full history, so a skipped hook only moves the failure to the PR.

## Pull requests

`main` is protected: nothing lands except through a pull request, and nobody can bypass it.

1. Branch from an up-to-date `main`: `git switch -c <type>/<topic> origin/main`.
2. Commit, run `just check`, push, and open a PR: `gh pr create`.
3. Title the PR as a Conventional Commit, `<type>(<scope>)?: <summary>` with `<type>` one of `build`, `chore`, `ci`, `docs`, `feat`, `fix`, `perf`, `refactor`, `revert`, `style`, `test` (e.g. `feat(import): read closed accounts`). The `pr-title` check enforces it.
4. Every CI job must pass, on a branch that is up to date with `main` (rebase or use *Update branch* when it isn't).
5. Squash-merge (the only method). The PR title becomes the commit subject on `main`; the branch is deleted on merge.

A CI job added to `.github/workflows/` is added to the required checks on `main` in the same change.

If the leak guard fails on a PR, the term is already on GitHub: remove it from the branch's history (rewrite the branch and force-push it, or close the PR and delete the branch), don't fix it with a follow-up commit. CI scans every ref it fetches, so a leaking branch fails other PRs until it's gone.

## The denylist

One term per line, matched case-insensitively as a whole word; a space matches any run of whitespace. A line starting with `re:` is a Python regex. Lines starting with `#` are comments. For example:

```text
# names
Alex Example
# account suffixes
1234
re:(?<![0-9a-z])[a-z]{1,12}1234(?![0-9a-z])
```

When a term is added, update the local file and the CI secret together: `gh secret set LEAK_DENYLIST < ~/.config/budget-leak-denylist.txt`. CI always redacts (`LEAK_GUARD_REDACT=1`). Findings name the term and the file; set `LEAK_GUARD_REDACT=1` to name the denylist line instead, for output that leaves the PC.
