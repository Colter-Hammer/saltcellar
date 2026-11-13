-- Balance adjustments, the rows each sync run touched, and the opening balances an import moved
-- (docs/spec/m2-bank-data.md#balances). Shipped migrations are never edited
-- (scripts/migrations.py): change the schema with a new one.
--
-- Beyond docs/spec/schema.sql *(agent)*:
-- - `sync_row_changes`: what a run added, changed or dropped on each account, kept with the run
--   (90 days), so a drift flag can list its **suspect rows**. The row's date, amount and bank text
--   are copied, since a dropped row is gone and a changed one may change again.
-- - `import_batch_openings`: an account's opening balance before an import moved it back over
--   older rows, so undoing the import puts it back.

-- Not a transaction: never in spending, budgets, Review or transfer matching. Undo keeps the row
-- so a later drift report can count how often drift went unexplained.
CREATE TABLE balance_adjustments (
  id           INTEGER PRIMARY KEY,
  account_id   INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  date         TEXT NOT NULL,
  amount_minor INTEGER NOT NULL,
  note         TEXT,
  accepted_by  INTEGER REFERENCES users(id),
  accepted_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  undone_at    TEXT,
  undone_by    INTEGER REFERENCES users(id)
) STRICT;
CREATE INDEX idx_balance_adjustments_account ON balance_adjustments(account_id);

CREATE TABLE sync_row_changes (
  sync_run_id    INTEGER NOT NULL REFERENCES sync_runs(id) ON DELETE CASCADE,
  account_id     INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  transaction_id INTEGER REFERENCES transactions(id) ON DELETE SET NULL,
  change         TEXT NOT NULL CHECK (change IN ('added','changed','dropped')),
  date           TEXT NOT NULL,
  amount_minor   INTEGER NOT NULL,
  description    TEXT NOT NULL
) STRICT;
CREATE INDEX idx_sync_row_changes_account ON sync_row_changes(account_id, sync_run_id);
CREATE INDEX idx_sync_row_changes_transaction ON sync_row_changes(transaction_id);

CREATE TABLE import_batch_openings (
  batch_id              INTEGER NOT NULL REFERENCES import_batches(id) ON DELETE CASCADE,
  account_id            INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  opening_balance_minor INTEGER NOT NULL,
  opening_date          TEXT NOT NULL,
  PRIMARY KEY (batch_id, account_id)
) STRICT;
