-- The household and the key/value settings (docs/spec/schema.sql).
-- Shipped migrations are never edited (scripts/migrations.py): change the schema with a new one.

CREATE TABLE household (                    -- Exactly one row, written by the setup wizard.
  id            INTEGER PRIMARY KEY CHECK (id = 1),
  currency_code TEXT NOT NULL,              -- ISO 4217; fixed once any transaction exists (enforce in code)
  timezone      TEXT NOT NULL,              -- IANA name, pre-filled from the browser at setup, editable later
  setup_done_at TEXT                        -- NULL until the wizard finishes; the setup code stops working then
) STRICT;

CREATE TABLE settings (                     -- In-app Settings; keys are listed in docs/spec/schema.sql.
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;
