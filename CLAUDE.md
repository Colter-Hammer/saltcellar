## Spec

The v1 spec is `docs/spec/` (start at `docs/spec/README.md`); the domain vocabulary is `GLOSSARY.md`.

## Agent skills

### Issue tracker

Implementation issues are GitHub Issues, mirrored as local markdown under `.scratch/<feature>/`; planning stays local. The repo is public: guard-check issue text before posting. See `docs/agents/issue-tracker.md`.

### Implementing an issue

The rules for building one, from what to read to the PR and the report: `docs/agents/implementing-an-issue.md`. A brief only needs the issue number. Run `just check-quiet`, not `just check`, so the output stays short.

### Triage labels

Default five-label vocabulary (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `GLOSSARY.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.
