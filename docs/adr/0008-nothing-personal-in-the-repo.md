# Nothing personal in the repo

The app repo contains no personal or deployment-specific information: no names, account numbers or suffixes, payees, IP addresses, domains, or details of the owner's infrastructure, in code, defaults, migrations, seed data, docs or tests. Test data is generated and fake. Planning notes stay in a git-ignored folder; the owner's home-server runbooks live in a separate local folder that is never pushed anywhere. This is enforced, not just intended: default-deny ignores for data files, `gitleaks` and a denylist check in pre-commit and pre-push hooks, and the same checks over full history in CI with no excluded paths. The denylist is itself personal, so it is kept outside the repo (a local file for hooks, a CI secret for CI) and the repo holds only the script that reads it.

This is both a privacy rule (the owner doesn't want any of it released, even in a private repo) and the portability rule: another household should be able to run the app with configuration alone.

## Consequences

- The spec in `docs/spec/` is self-contained and generic; decisions specific to the owner's deployment are recorded outside the repo.
- Adding a denylist term needs both the local file and the CI secret updated.
- A hook can be skipped locally, which is why CI repeats every check and the project's agent settings deny `--no-verify`.
