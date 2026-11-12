-- The bank link each feed account belongs to (docs/spec/m2-bank-data.md#errors): the feed's
-- `conn_id`, which `con.*` errors name and the Banks list in Settings groups by. NULL until a
-- response that names it is recorded, and for a feed that doesn't name bank links *(agent)*.
-- Shipped migrations are never edited (scripts/migrations.py): change the schema with a new one.

ALTER TABLE connection_accounts ADD COLUMN bank_link_id TEXT;
