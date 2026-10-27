# Milestone 2: Bank data in

> **Placeholder.** The sections for this milestone are written separately, once the bank sync design finishes its live observation of the provider feed. Until then, this page lists what they will cover and what's already fixed elsewhere in the spec. Don't start milestone 2 from this page.

**Goal:** bank data arrives on its own every day, files fill the gaps, and anything can be entered by hand.

## What the sections will cover

- **SimpleFIN sync.** Claiming the setup token in Settings (the access URL is stored encrypted in the database, never displayed again, never in the repo or image); a daily scheduled sync, Sync now, retries, run history; needs-attention, stale and paused states; pending → posted; upserts and back-fill; balances, balance snapshots and drift; balance adjustments.
- **CSV/QFX import.** A generic CSV column mapper (date, amount or debit/credit, description, optional bank category; date format; sign convention) saved per account and reused; OFX/QFX/QBO parsed directly; no hard-coded bank presets; a preview before commit; each file an undoable Import batch.
- **Manual entry**, and sync adopting manual and imported rows.
- **Import from Actual, accounts step:** matching Actual accounts to synced accounts by provider account id, and de-duplicating the overlap between imported and synced rows.
- **Sync health in the UI:** one status spot (phone header pill, desktop sidebar foot) opening a sheet with every problem and its fix; a ⚠ marker on problem accounts; the paused banner on every screen.
- The ADR for SimpleFIN as the provider, the sync tables in [schema.sql](schema.sql) (marked `[sync]`), and the sync terms in the glossary.
- The sync-dependent part of milestone 3's transfer matching (windows, strength rules, timing).

## Already fixed elsewhere

- The pipeline never overwrites a category a person set ([milestone 3](m3-categorization-review.md#user-choices-win)).
- Transfer detection runs before rules ([milestone 3](m3-categorization-review.md#the-pipeline)).
- Settings lives in the database, not env ([Foundations › Configuration](foundations.md#configuration)).
- Balance snapshots, drift, opening balances, import batches, Connection and Manual account are defined in the [glossary](../../GLOSSARY.md).
