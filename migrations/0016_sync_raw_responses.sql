-- Kept raw bank responses (docs/spec/m2-bank-data.md#sync-runs): only while the household turns
-- on "Keep raw bank responses for troubleshooting", sealed with BUDGET_SECRET_KEY, pruned after
-- 7 days. Shipped migrations are never edited (scripts/migrations.py): change the schema with a
-- new one.

CREATE TABLE sync_raw_responses (
  sync_run_id INTEGER NOT NULL REFERENCES sync_runs(id) ON DELETE CASCADE,
  seq         INTEGER NOT NULL,
  received_at TEXT NOT NULL,
  body        BLOB NOT NULL,                -- sealed with BUDGET_SECRET_KEY; never logged
  PRIMARY KEY (sync_run_id, seq)
) STRICT;
CREATE INDEX idx_sync_raw_responses_received ON sync_raw_responses(received_at);
