-- Which import batch created an account, so undoing the batch can delete the accounts it made
-- (docs/spec/m1-ledger-import.md#settings--imports). NULL for an account no batch created.
-- A plain column with no foreign key, like `connection_id` *(agent)*.
-- Shipped migrations are never edited (scripts/migrations.py): change the schema with a new one.

ALTER TABLE accounts ADD COLUMN created_by_batch_id INTEGER;
