-- Rows the household chose to keep although the bank stopped sending them
-- (docs/spec/m2-bank-data.md#rows-the-bank-stops-sending): **Keep** on a `not_seen_at_bank` row,
-- or **Restore** from Recently dropped by bank. Sync no longer flags them `not_seen_at_bank` nor
-- counts their misses, so the household's answer stands *(agent)*.
-- Shipped migrations are never edited (scripts/migrations.py): change the schema with a new one.

ALTER TABLE transactions ADD COLUMN kept_by_household INTEGER NOT NULL DEFAULT 0;
