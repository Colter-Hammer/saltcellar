# Issue tracker: GitHub Issues, mirrored locally

Implementation issues live on GitHub (`gh issue …`). Planning stays in local markdown under `.scratch/` (gitignored).

**The repo is public.** Every issue title, body and comment is published the moment it's posted, and the leak guard never sees it. Before posting any of them, write the text to a file and run `python3 scripts/guard.py leak msg <file>`; post only if it passes. Never put real accounts, payees, amounts, people or export rows in an issue. Refer to the owner as "the owner", not by name.

## Implementation issues (GitHub)

- One issue per ticket, labelled with its triage role (see `triage-labels.md`) and the milestone (e.g. `M1 ledger import`).
- **Blocking**: the body has a `Blocked by: #N, #N` line, and the same issues are linked with GitHub's native "blocked by" relation (`gh api -X POST repos/{owner}/{repo}/issues/<n>/dependencies/blocked_by -F issue_id=<blocker's id>`, where the id comes from `gh api repos/{owner}/{repo}/issues/<blocker>`, not the number). An issue is unblocked when every blocker is closed.
- **Next work**: the lowest-numbered open `ready-for-agent` issue whose blockers are all closed.
- **Working it**: branch, build, and open a PR whose body has `Closes #N`. Squash-merging the PR into `main` closes the issue; don't close it by hand.
- **Progress notes** go on the issue as comments (`gh issue comment`), guard-checked first. Don't paste things only true on the owner's PC (ports, paths outside the repo) unless they matter to the work.

## Local mirror

Every GitHub issue keeps a local copy at `.scratch/<feature-slug>/issues/<NN>-<slug>.md` (the spec is `.scratch/<feature-slug>/spec.md`). The local file has a `GitHub: <issue url>` line under its title. GitHub is the live copy; the local file is the full record, so keep it current: when the issue changes state or gets a comment, update the local `Status:` line and append the comment under `## Comments` at the bottom.

## When a skill says "publish to the issue tracker"

Write the local file first (`.scratch/<feature-slug>/issues/<NN>-<slug>.md`, numbered from `01`, one file per ticket, never a combined file). Then guard-check it and create the GitHub issue with `gh issue create`, adding the triage label and milestone, and put the issue URL in the local file's `GitHub:` line.

## When a skill says "fetch the relevant ticket"

`gh issue view <N> --comments`. The user may pass the issue number, the URL, or the local path (read its `GitHub:` line).

## Wayfinding operations (local only)

Used by `/wayfinder`. Planning maps and their tickets stay local and are never posted; they hold research on real data.

- **Map**: `.scratch/<effort>/map.md` (the Notes / Decisions-so-far / Fog body).
- **Child ticket**: `.scratch/<effort>/issues/NN-<slug>.md`, numbered from `01`, with the question in the body. A `Type:` line records the ticket type (`research`/`prototype`/`grilling`/`task`); a `Status:` line records `claimed`/`resolved`.
- **Blocking**: a `Blocked by: NN, NN` line near the top. A ticket is unblocked when every file it lists is `resolved`.
- **Frontier**: scan `.scratch/<effort>/issues/` for files that are open, unblocked, and unclaimed; first by number wins.
- **Claim**: set `Status: claimed` and save before any work.
- **Resolve**: append the answer under an `## Answer` heading, set `Status: resolved`, then append a context pointer (gist + link) to the map's Decisions-so-far in `map.md`.
