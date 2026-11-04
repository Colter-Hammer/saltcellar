-- Each category group's Lucide icon name (docs/spec/visual-system.md#group-color-and-icon), like
-- `heart-pulse`: checked for shape by the API, not against a list. NULL shows `folder`.
-- Shipped migrations are never edited (scripts/migrations.py): change the schema with a new one.

ALTER TABLE category_groups ADD COLUMN icon TEXT;
