-- Saved CSV column mappings (docs/spec/m2-bank-data.md#csv-and-ofxqfx-import, docs/spec/schema.sql).
-- Shipped migrations are never edited (scripts/migrations.py): change the schema with a new one.
--
-- `import_batches.mapping_id` stays a plain column with no foreign key, as milestone 1 left it
-- (rebuilding the table to add the key isn't worth it): a mapping is only deleted with its
-- account, whose batches go with it *(agent)*.

CREATE TABLE import_mappings (
  id               INTEGER PRIMARY KEY,
  account_id       INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  header_signature TEXT NOT NULL,
  config           TEXT NOT NULL,           -- JSON: date col, post_date col?, description col, amount col OR
                                            --   debit/credit cols, flip_sign, date_format, category col?, memo col?,
                                            --   skip_rows
  updated_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  UNIQUE (account_id, header_signature)
) STRICT;
