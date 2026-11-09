-- Every account the bank connection reports (docs/spec/m2-bank-data.md#accounts), waiting in the
-- new accounts prompt until tracked or ignored. Shipped migrations are never edited
-- (scripts/migrations.py): change the schema with a new one.
--
-- Two columns beyond the spec's first draft *(agent)*: `currency`, so an account in another
-- currency than the household's is recorded and shown as refused, never tracked; and
-- `last_seen_at`, so an id that stopped arriving can be told from one still coming (the
-- "This is the same as…" suggestion).

CREATE TABLE connection_accounts (
  connection_id       INTEGER NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
  external_account_id TEXT NOT NULL,
  provider_name       TEXT NOT NULL,
  institution         TEXT,
  currency            TEXT NOT NULL,
  decision            TEXT NOT NULL DEFAULT 'pending' CHECK (decision IN ('pending','tracked','ignored')),
  account_id          INTEGER UNIQUE REFERENCES accounts(id),
  first_seen_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  last_seen_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (connection_id, external_account_id),
  CHECK ((decision = 'tracked') = (account_id IS NOT NULL))
) STRICT;
