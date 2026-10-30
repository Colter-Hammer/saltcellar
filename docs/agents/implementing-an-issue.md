# Implementing an issue

The rules every agent follows when it builds an implementation issue, so a brief only has to name the issue. Where to find issues and how to keep their local copies: `issue-tracker.md`.

## Read first, and only this

- The issue (`gh issue view <N>`) and its local copy. Local files are numbered one lower than GitHub issues (GitHub #19 is `.scratch/<feature>/issues/18-….md`); match on the `GitHub:` line.
- The spec section the issue links, `GLOSSARY.md` for its words, and `docs/standards.md` for the checks.
- The code the issue builds on: the PRs that closed its blockers (`gh issue view <blocker> --json closedByPullRequestsReferences`). Don't read further than the work needs.

## Build

- Decide fine details yourself and mark each with *(agent)* in the spec and the PR body. Don't stop to ask.
- Fake data only, in tests, fixtures and screenshots.
- Regenerate API files with `just gen-api`; never edit generated files by hand.
- Migrations are numbered in order (`ls migrations/`); add one only when the work needs it.
- Write transactions in `db` start with `pool.begin_with("BEGIN IMMEDIATE")`, never a plain `begin()` (see `db::connect`).
- If port 8080 is taken, run the dev API with `BUDGET_LISTEN=127.0.0.1:8090`. e2e uses 8180.
- Playwright tests that need imported data run after the `import-commit` project.

## Check

- While iterating, run only the tests you're changing (`cargo nextest run -p <crate> <filter>`, `npm --prefix web run test -- <file>`).
- Before pushing, run `just check-quiet` once. It prints one line per check and only a failing check's last lines; the full logs are in `target/check-quiet/`.
- Commit everything before pushing: the pre-push hook runs `just e2e` (under `flock /tmp/saltcellar-e2e.lock`, so parallel pushes take turns) and refuses a push with uncommitted tracked changes. Never `--no-verify`.

## Open the PR

- Branch `feat/<slug>` (or `fix/`, `chore/`), conventional-commit title, `Closes #<N>` in the body.
- No attribution lines in commits or PRs: no `Co-Authored-By`, no "Generated with".
- The repo is public: write the body to a file and run `python3 scripts/guard.py leak msg <file>` before `gh pr create`.
- Wait for CI and fix failures until every check passes. Don't merge; the owner approves merges.
- Don't change the GitHub ruleset. Say so in the report if you added a CI job.
- Update the local copy in the main checkout, not a worktree: `Status: in-review (PR #N)`, tick the acceptance criteria met, and add a dated `- <date> (agent): …` line under `## Comments`. Don't touch the main checkout's git state.

## Report

Keep it short: what was built, deviations and unsure assumptions, the details you placed, the owner's steps to check it in the browser, and a last line `PR <url>, head <sha>, checks <state>`.
