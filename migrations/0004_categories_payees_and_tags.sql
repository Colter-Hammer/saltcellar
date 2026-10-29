-- Category groups and categories, payees and their aliases, and tags (docs/spec/schema.sql).
-- Shipped migrations are never edited (scripts/migrations.py): change the schema with a new one.
-- The starter categories and always-review payees aren't seeded here: applying them is a choice
-- on the Categories screen, so they live in code (domain::categories).

CREATE TABLE category_groups (              -- A container only; never holds transactions.
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL UNIQUE,
  kind       TEXT NOT NULL DEFAULT 'expense' CHECK (kind IN ('expense','income')),
  sort_order INTEGER NOT NULL DEFAULT 0,
  hidden     INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE TABLE categories (                   -- In exactly one group, which can't be deleted while it holds any.
  id         INTEGER PRIMARY KEY,
  group_id   INTEGER NOT NULL REFERENCES category_groups(id),
  name       TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  hidden     INTEGER NOT NULL DEFAULT 0,    -- hide instead of delete once transactions use it
  UNIQUE (group_id, name)
) STRICT;

CREATE TABLE payees (                       -- The merchant/counterparty as the household thinks of it.
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL UNIQUE,
  review_mode TEXT NOT NULL DEFAULT 'auto'
                CHECK (review_mode IN ('auto','always','never'))
                -- auto:   normal cascade; mixed or often-split history → Review
                -- always: never auto-categorized; Review with suggestions (starter set: docs/spec/m1)
                -- never:  apply the top history pick even if mixed; only a first sighting goes to Review
) STRICT;

CREATE TABLE payee_aliases (                -- Normalized bank text → payee; merging payees = repointing aliases.
  normalized_text TEXT PRIMARY KEY,
  payee_id        INTEGER NOT NULL REFERENCES payees(id) ON DELETE CASCADE
) STRICT;

CREATE TABLE tags (
  id   INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE
) STRICT;
