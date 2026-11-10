-- Sync runs and their per-account results, the bank rows the household deleted, and when each
-- feed account was back-filled (docs/spec/m2-bank-data.md#sync-runs). Shipped migrations are
-- never edited (scripts/migrations.py): change the schema with a new one.
--
-- Beyond docs/spec/schema.sql *(agent)*: the runs go with their connection (ON DELETE CASCADE),
-- since Disconnect deletes it; a result goes with its account, which undoing an import can
-- delete; and `connection_accounts.backfilled_at` marks a feed account whose 90-day back-fill is
-- done, so a newly tracked one (or one moved onto a new id) is back-filled once.

CREATE TABLE sync_runs (
  id            INTEGER PRIMARY KEY,
  connection_id INTEGER NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
  trigger       TEXT NOT NULL CHECK (trigger IN ('scheduled','manual','retry','catch_up','backfill')),
  started_at    TEXT NOT NULL,
  finished_at   TEXT,
  requests      INTEGER NOT NULL DEFAULT 0,  -- Bridge requests made; app cap is 12 per rolling 24h
  added         INTEGER NOT NULL DEFAULT 0,
  updated       INTEGER NOT NULL DEFAULT 0,
  error_code    TEXT,
  error         TEXT                         -- sanitized; never amounts, merchants or the access URL
) STRICT;
CREATE INDEX idx_sync_runs_started ON sync_runs(started_at);

CREATE TABLE sync_account_results (
  sync_run_id           INTEGER NOT NULL REFERENCES sync_runs(id) ON DELETE CASCADE,
  account_id            INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  ok                    INTEGER NOT NULL,    -- 0 = keeps its last data; no miss counts or drift move this run
  error_code            TEXT,                -- errlist code, or 'absent', 'bad_amount', 'bad_date' *(agent)*
  error                 TEXT,
  provider_balance_date TEXT,
  PRIMARY KEY (sync_run_id, account_id)
) STRICT;

CREATE TABLE deleted_bank_rows (            -- Sync skips these ids so the daily window doesn't bring them back.
  account_id   INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  external_id  TEXT NOT NULL,
  deleted_by   INTEGER REFERENCES users(id),
  deleted_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (account_id, external_id)
) STRICT;

ALTER TABLE connection_accounts ADD COLUMN backfilled_at TEXT;
