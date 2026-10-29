# Saltcellar

A self-hosted budgeting app for one household: your bank data comes in, transactions get categorized with as little effort as possible, and spending is compared against the targets you set.

People say someone is "worth their salt", and a salt cellar is where the salt was kept. Saltcellar is the place to keep track of what your money is worth and to plan where it goes.

> **Status: early development, not ready for use.** Milestone 1 of 6 (the ledger and the import from Actual Budget) is being built. Nothing here is stable yet, including the database.

## What it does

- **Tracks spending against targets, not envelopes.** A budget is a cap for a category group with limits for the categories inside it, monthly or over a window of several months (for example, a quarterly water bill). Nothing rolls over, except in funds.
- **Funds carry over.** Each person's play money, and sinking funds that save toward known bills, build up month after month. A fund can be kept on paper or held in a real account.
- **Categorizes as much as it can and asks about the rest.** It uses payee history, your rules and a local classifier. Anything it isn't sure about goes to a review inbox you work through one transaction at a time, with quick splits. Payees like big-box stores can be set to always come to review.
- **Shows savings and investments as balances.** Tracking accounts show their balance over time and count as money saved, never as spending. Savings goals can be a contribution per month or a balance to reach.
- **Brings in bank data** through [SimpleFIN Bridge](https://beta-bridge.simplefin.org/), CSV/QFX files, or manual entry. It only reads; it never moves money.
- **Imports from Actual Budget,** so your history comes with you and teaches the categorizer.
- **Works on phone and desktop,** as an installable web app with a read-only offline view.
- **Stays yours:** one SQLite database on your own machine, encrypted backups, and no cloud service apart from your bank-data provider.

## Stack

Rust (axum, sqlx) and SQLite on the server; Vue 3, TypeScript, Tailwind and shadcn-vue in the browser. It runs with Docker Compose behind your own HTTPS reverse proxy, on a home network or a private VPN. It isn't meant for the public internet.

## How it's built

The code is written with [Claude Code](https://claude.com/claude-code), Anthropic's AI coding agent, working from a written spec. The design is mine: I decide the architecture, the behaviour and every trade-off, and I record the hard-to-reverse ones as decisions in [docs/adr/](docs/adr/). Claude writes the code against that, and I review every change before it is merged. Changes also have to pass the same checks and merge gate as any other contribution.

## Docs

- [Spec](docs/spec/README.md): what is being built, milestone by milestone.
- [Glossary](GLOSSARY.md): the words the app uses, like budget, fund and tracking account.
- [Decisions](docs/adr/): the hard-to-reverse choices and why they were made.
- [Deploying](docs/deploy.md): Docker Compose, the environment variables and the reverse proxy.
- [Contributing](CONTRIBUTING.md): tools, checks and how changes get merged.

## License

Copyright (C) 2026 Colter-Hammer

Saltcellar is free software: you can redistribute it and/or modify it under the terms of the GNU Affero General Public License as published by the Free Software Foundation, version 3. It is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See [LICENSE](LICENSE) for the full text.

In short: use it, change it, share it. If you run a changed version for other people, you have to share your changes under the same license.
