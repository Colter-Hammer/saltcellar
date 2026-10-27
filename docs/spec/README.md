# Budgeting v1 spec

A self-hosted, Mint-style budgeting app for one household: bank data comes in, transactions get categorized with as little effort as possible, and spending is compared against targets. Tracking-style budgeting, not envelopes: budgets compare a target with actual spending and nothing carries over, except in funds ([ADR-0003](../adr/0003-funds-carry-over.md)).

This spec is enough to build milestone 1 without asking the owner anything, and describes every later milestone except the bank sync parts, which are added separately (see [Milestone 2](m2-bank-data.md)). Visual styling is not in it: the screens below are layouts, and a styling pass happens once milestone 1 runs on real data.

## How to read it

| Doc | What it covers |
|---|---|
| [Foundations](foundations.md) | Stack, architecture, conventions, configuration, auth, coding standards, app shell. Read first. |
| [Milestone 1: Ledger + Import from Actual](m1-ledger-import.md) | Setup wizard, login, categories, accounts, transactions, the Actual importer. |
| [Milestone 2: Bank data in](m2-bank-data.md) | Placeholder for SimpleFIN sync, CSV/QFX import and manual entry. |
| [Milestone 3: Categorization + Review](m3-categorization-review.md) | The categorization pipeline, Review, transfers, rules, history re-check, accuracy stats. |
| [Milestone 4: Budgets, Overview, reports](m4-budgets-reports.md) | Budget setup and screen, Overview, Spending, Trends, Cash flow. |
| [Milestone 5: Funds + savings goals](m5-funds-goals.md) | Play money, sinking funds, savings goals. |
| [Milestone 6: Launch](m6-launch.md) | Backups, reminder, export, invites, trusted-header auth, PWA, release images. |
| [schema.sql](schema.sql) | The target schema, annotated with the milestone that first needs each table. |
| [/GLOSSARY.md](../../GLOSSARY.md) | Domain vocabulary. Use these terms in code, UI copy, issues and tests. |
| [docs/adr/](../adr/) | Hard-to-reverse decisions and why. |

Decisions are stated plainly here; the discussion behind them isn't part of the repo. Where a detail was placed by the agent assembling the spec rather than agreed with the owner, it is marked *(agent)*. Those are reasonable defaults, open to change without ceremony; everything unmarked was agreed.

## Principles

- **Own ledger.** The app keeps its own transactions, categories and history in its own database. Bank data flows in; nothing flows back. The app never moves money.
- **Tracking, not envelopes.** No "to be budgeted", no rollover pools. Funds are the one carry-over exception.
- **Portable.** Another household could run it on their own machine with configuration alone. No owner names, domains, IPs, banks or infrastructure in code, defaults, migrations, seed data or docs in this repo ([ADR-0008](../adr/0008-nothing-personal-in-the-repo.md)).
- **User choices win.** Anything a person set by hand is never overwritten by import, rules, sync or re-checks.
- **Phone and desktop equally.** Mobile-first layouts that grow into desktop ones.
- **Strict from the first commit.** Standards are enforced by checks, not goodwill ([Foundations › Coding standards](foundations.md#coding-standards)).
- **Small, browser-checkable steps.** The owner mostly QA's generated code; every milestone, and ideally every issue, ends with something to check in a browser.

## Users and scale

One household, one shared budget, any number of equal logins (two in practice), no privacy split between them. Tens of thousands of transactions at most. Reachable on the home network only, later also through a private VPN; never on the public internet.

## Milestones at a glance

The app runs on the owner's PC (`docker compose up`, localhost) through milestone 5, while the old budgeting tool stays the real budget, so losing app data before launch only costs a re-import. It moves to the home server at milestone 6.

1. **Ledger + Import from Actual.** Repo, guards and checks first; then skeleton, setup, login, categories, read-only accounts and transactions, and the Actual importer. Done when the owner's real export shows correctly, with balances matching Actual.
2. **Bank data in.** SimpleFIN sync, CSV/QFX import, manual entry.
3. **Categorization + Review.** Pipeline, Review inbox with splits, Transfers screen, rules and rule suggestions, history re-check, accuracy stats.
4. **Budgets + Overview + reports.**
5. **Funds + savings goals.**
6. **Launch.** Backups and Download backup, review reminder, CSV export, invites and the second login, PWA, release images, move to the server (the move is the first restore drill).

After launch, in order and with no new app code: VPN access for phones away from home, an off-site backup copy, then switching the home deployment to trusted-header auth.

## Out of scope for v1

- Envelope budgeting, rollover pools, "to be budgeted".
- Investment holdings, trade-level data, performance or returns (investment accounts are balances only).
- A native mobile app (responsive web + PWA only).
- Bill pay, credit monitoring, anything that moves money.
- Public internet exposure.
- Multiple currencies, FX, languages other than English.
- Post-v1 reports, for which the data is kept: net worth, sinking fund progress, play money remaining, merchant report, budget history timeline, drift report.
- Alerts beyond the review reminder and the backup / restore-drill alerts (over budget, large charge, broken sync: leave room behind the notifier seam).
- Order-history matching (e.g. splitting an online-store charge by its order).
- Receipt photos, typed natural-language budget entry.
- Publishing for others: public images, a licence, end-user docs, support.
