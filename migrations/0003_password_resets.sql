-- One-time password reset links (docs/spec/schema.sql).
-- Shipped migrations are never edited (scripts/migrations.py): change the schema with a new one.

CREATE TABLE password_resets (              -- Issued by the `reset-password` CLI command; by another login in Settings from milestone 6.
  token_hash TEXT PRIMARY KEY,              -- hash of the link's token, never the raw token
  user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created_by INTEGER REFERENCES users(id),  -- NULL = issued by the `reset-password` CLI command
  expires_at TEXT NOT NULL,                 -- 24 hours
  used_at    TEXT
) STRICT;
