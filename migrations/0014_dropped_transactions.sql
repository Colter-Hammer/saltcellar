-- Pending rows the bank stopped sending, kept when sync removes them
-- (docs/spec/m2-bank-data.md#rows-the-bank-stops-sending). Shipped migrations are never edited
-- (scripts/migrations.py): change the schema with a new one.
--
-- Beyond docs/spec/schema.sql *(agent)*: the rows go with their account (ON DELETE CASCADE), like
-- `deleted_bank_rows`, since undoing an import can delete an account; and an index for the
-- "Recently dropped by bank" list.

CREATE TABLE dropped_transactions (
  id           INTEGER PRIMARY KEY,
  account_id   INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  snapshot     TEXT NOT NULL,               -- JSON of the row with its splits and tags, enough to restore it
  user_edited  INTEGER NOT NULL,            -- 1 = listed under "Recently dropped by bank", undoable
  dropped_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  restored_at  TEXT                         -- *(agent)*
) STRICT;
CREATE INDEX idx_dropped_at ON dropped_transactions(dropped_at);
