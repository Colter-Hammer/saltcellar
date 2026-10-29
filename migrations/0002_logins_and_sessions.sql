-- Logins, their sessions and the login throttle (docs/spec/schema.sql).
-- Shipped migrations are never edited (scripts/migrations.py): change the schema with a new one.

CREATE TABLE users (                        -- A login. All equal: no admin role, no cap on the count.
  id             INTEGER PRIMARY KEY,
  username       TEXT NOT NULL UNIQUE,
  display_name   TEXT NOT NULL,
  password_hash  TEXT,                      -- argon2id; built-in login mode only (NULL in trusted-header mode)
  remote_user    TEXT UNIQUE,               -- trusted-header mode: the header value that maps to this login
  display_locale TEXT,                      -- BCP 47; NULL = use the browser's locale
  week_start     INTEGER CHECK (week_start BETWEEN 0 AND 6), -- 0 = Sunday; NULL = from the locale
  created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  removed_at     TEXT                       -- removed by another login; kept so updated_by references stay valid
) STRICT;

CREATE TABLE sessions (                     -- Built-in mode. 30-day sliding expiry; deleting rows = log out.
  token_hash   TEXT PRIMARY KEY,            -- hash of the cookie token, never the raw token
  user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  last_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  expires_at   TEXT NOT NULL                -- last_seen_at + 30 days, pushed forward on use
) STRICT;

CREATE TABLE login_failures (               -- 5 failures in 15 min pause that username for 15 min.
  username   TEXT NOT NULL,                 -- as typed, so unknown names are throttled too; `setup` for the setup code
  failed_at  TEXT NOT NULL
) STRICT;
CREATE INDEX idx_login_failures ON login_failures(username, failed_at);
