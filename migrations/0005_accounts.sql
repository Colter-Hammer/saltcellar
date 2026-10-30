-- Accounts (docs/spec/schema.sql). The Actual importer creates them in milestone 1.
-- Shipped migrations are never edited (scripts/migrations.py): change the schema with a new one.
-- `connection_id` is a plain nullable column with no foreign key yet: `connections` arrives with
-- bank sync in milestone 2, which rebuilds this table to add the key *(agent)*.

CREATE TABLE accounts (
  id                  INTEGER PRIMARY KEY,
  name                TEXT NOT NULL UNIQUE,
  institution         TEXT,
  type                TEXT NOT NULL CHECK (type IN
                        ('checking','savings','credit_card','investment','loan','cash','other')),
  role                TEXT NOT NULL CHECK (role IN ('budgeted','tracking')),
                        -- budgeted: activity counts toward budgets (checking, shared cards, play-money accounts)
                        -- tracking: balance over time only; money moved in counts as Saved
  has_transactions    INTEGER NOT NULL DEFAULT 1, -- 0 = balances only (investments, loans): transfers to it are one-legged
  in_net_worth        INTEGER NOT NULL DEFAULT 1, -- for the post-v1 net worth report
  closed              INTEGER NOT NULL DEFAULT 0,
  sort_order          INTEGER NOT NULL DEFAULT 0,
  connection_id       INTEGER,                -- REFERENCES connections(id) from milestone 2
  external_account_id TEXT,                 -- provider account id. The Actual import sets it (with connection_id NULL)
                                            -- from Actual's own bank-sync id, so a later sync attaches the account
                                            -- as already tracked.
  -- Opening balance: derived, never a transaction. From the Actual import (M1): the account's Actual balance
  -- minus the imported rows, dated the day before the earliest imported row. On first sync (M2): first bank
  -- balance minus back-filled rows. When older history arrives, the date moves back and the amount is
  -- recomputed so drift is unchanged. NULL for balances-only accounts.
  opening_balance_minor INTEGER,
  opening_date          TEXT,
  balance_includes_pending INTEGER,         -- [sync] does the bank's balance count pending rows? NULL = unknown
  UNIQUE (connection_id, external_account_id),
  CHECK ((opening_balance_minor IS NULL) = (opening_date IS NULL))
) STRICT;
