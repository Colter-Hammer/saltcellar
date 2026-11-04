-- no-transaction
-- The bank connection (docs/spec/m2-bank-data.md#the-connection), and the foreign key from
-- `accounts.connection_id` that milestone 1 left off. Shipped migrations are never edited
-- (scripts/migrations.py): this one adds the table and rebuilds `accounts`.
--
-- SQLite can't add a foreign key to a column, so `accounts` is rebuilt the way its documentation
-- says (https://sqlite.org/lang_altertable.html#otheralter). With foreign keys on, dropping the
-- old table would delete its rows one by one and cascade into the tables that point at it
-- (`balance_snapshots` is ON DELETE CASCADE), so this runs outside sqlx's transaction, with
-- foreign keys off, and opens its own. Every row, the unique constraints and the column order
-- carry over; `accounts` has no separate indexes. `legacy_alter_table` stops the rename from
-- touching the views over `accounts` while the table is out of the way *(agent)*.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN IMMEDIATE;

CREATE TABLE connections (
  id               INTEGER PRIMARY KEY,
  provider         TEXT NOT NULL CHECK (provider IN ('simplefin')),
  label            TEXT NOT NULL,
  secret_encrypted BLOB NOT NULL,           -- XChaCha20-Poly1305 with BUDGET_SECRET_KEY, which lives outside the DB
  last_sync_at     TEXT,
  last_error       TEXT,
  status           TEXT NOT NULL DEFAULT 'ok' CHECK (status IN ('ok','token_revoked','unpaid','key_mismatch')),
                     -- token_revoked (403/gen.auth) or unpaid (402): all syncing paused until a new
                     -- setup token is claimed, which replaces secret_encrypted on this same row.
                     -- key_mismatch: BUDGET_SECRET_KEY can't decrypt the secret; same fix *(agent)*
  paused_at        TEXT
) STRICT;

CREATE TABLE accounts_rebuilt (
  id                  INTEGER PRIMARY KEY,
  name                TEXT NOT NULL UNIQUE,
  institution         TEXT,
  type                TEXT NOT NULL CHECK (type IN
                        ('checking','savings','credit_card','investment','loan','cash','other')),
  role                TEXT NOT NULL CHECK (role IN ('budgeted','tracking')),
  has_transactions    INTEGER NOT NULL DEFAULT 1,
  in_net_worth        INTEGER NOT NULL DEFAULT 1,
  closed              INTEGER NOT NULL DEFAULT 0,
  sort_order          INTEGER NOT NULL DEFAULT 0,
  connection_id       INTEGER REFERENCES connections(id),
  external_account_id TEXT,
  opening_balance_minor INTEGER,
  opening_date          TEXT,
  balance_includes_pending INTEGER,
  created_by_batch_id INTEGER,
  UNIQUE (connection_id, external_account_id),
  CHECK ((opening_balance_minor IS NULL) = (opening_date IS NULL))
) STRICT;

-- No account can point at a connection yet, so the new key can't be violated.
INSERT INTO accounts_rebuilt
  (id, name, institution, type, role, has_transactions, in_net_worth, closed, sort_order,
   connection_id, external_account_id, opening_balance_minor, opening_date,
   balance_includes_pending, created_by_batch_id)
SELECT
   id, name, institution, type, role, has_transactions, in_net_worth, closed, sort_order,
   NULL, external_account_id, opening_balance_minor, opening_date,
   balance_includes_pending, created_by_batch_id
FROM accounts;

DROP TABLE accounts;
ALTER TABLE accounts_rebuilt RENAME TO accounts;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;
