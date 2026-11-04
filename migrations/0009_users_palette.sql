-- Each login's visual-system palette (docs/spec/visual-system.md#palettes-and-modes): an id like
-- `sage`, checked for shape by the API, not against a list. NULL = the default.
-- Shipped migrations are never edited (scripts/migrations.py): change the schema with a new one.

ALTER TABLE users ADD COLUMN palette TEXT;
