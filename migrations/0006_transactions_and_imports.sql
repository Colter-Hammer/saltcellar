-- Transactions with their split lines, tags and transfers, balance snapshots, import batches and
-- their rows, and the M1 view on them (docs/spec/schema.sql). The Actual importer's commit step
-- writes them in milestone 1.
-- Shipped migrations are never edited (scripts/migrations.py): change the schema with a new one.
-- Columns pointing at tables that arrive with bank sync in milestone 2 (`sync_runs`,
-- `import_mappings`) are plain nullable columns with no foreign key yet, as `accounts.connection_id`
-- is; milestone 2 rebuilds these tables to add the keys *(agent)*.

CREATE TABLE transactions (
  id                  INTEGER PRIMARY KEY,
  account_id          INTEGER NOT NULL REFERENCES accounts(id),
  date                TEXT NOT NULL,
  amount_minor        INTEGER NOT NULL,
  status              TEXT NOT NULL DEFAULT 'posted' CHECK (status IN ('pending','posted')),
                        -- pending counts toward budgets immediately
  payee_id            INTEGER REFERENCES payees(id),
  raw_description     TEXT NOT NULL,        -- exactly what the bank sent; never edited
  bank_category       TEXT,                 -- [sync] the bank's own label from an imported file; a rule input only
  memo                TEXT,                 -- Actual import: Actual's notes
  category_id         INTEGER REFERENCES categories(id),
                        -- NULL when split, a transfer leg, or uncategorized
  exclude_from_budget INTEGER NOT NULL DEFAULT 0,
  categorized_by      TEXT CHECK (categorized_by IN ('user','rule','history','import','transfer')),
                        -- who set category_id (or, for 'transfer', made it a transfer leg).
                        -- The pipeline, rules and re-checks never overwrite 'user'.
  categorization_reason TEXT,               -- shown on the review card and the detail panel
  source              TEXT NOT NULL CHECK (source IN ('provider','import','manual')),
  external_id         TEXT,                 -- provider transaction id; the Actual import copies Actual's
  dedupe_key          TEXT NOT NULL,        -- [sync]
  last_seen_at        TEXT,                 -- [sync]
  pending_miss_count  INTEGER NOT NULL DEFAULT 0, -- [sync]
  sync_flag           TEXT CHECK (sync_flag IN ('possible_duplicate','not_seen_at_bank','amount_changed_split',
                                             'possible_manual_match')), -- [sync]
  possible_duplicate_of INTEGER REFERENCES transactions(id), -- [sync]
  awaiting_transfer_until TEXT,             -- [sync/M3] transfer-like leg with no partner yet: out of Review until then
  review_quiet        INTEGER NOT NULL DEFAULT 0,
                        -- imported row dated > 90 days before its import that the pipeline left uncategorized:
                        -- stays out of Review, listed under the "Imported, uncategorized" filter instead
  created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_by          INTEGER REFERENCES users(id), -- last login to edit; NULL = system (sync, rules, import)
  UNIQUE (account_id, external_id)
) STRICT;
CREATE INDEX idx_txn_account_date  ON transactions(account_id, date);
CREATE INDEX idx_txn_category_date ON transactions(category_id, date);
CREATE INDEX idx_txn_payee         ON transactions(payee_id);
CREATE INDEX idx_txn_dedupe        ON transactions(account_id, dedupe_key);

CREATE TABLE transaction_splits (           -- Enforce in code: lines sum to the parent amount.
  id             INTEGER PRIMARY KEY,
  transaction_id INTEGER NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
  category_id    INTEGER NOT NULL REFERENCES categories(id),
  amount_minor   INTEGER NOT NULL,
  memo           TEXT
) STRICT;

CREATE TABLE transaction_tags (
  transaction_id INTEGER NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
  tag_id         INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (transaction_id, tag_id)
) STRICT;

-- A move between our own accounts. Two legs normally; one leg when the other account has no
-- transaction data. Enforce in code: legs are in the named accounts, from-leg = −(to-leg), legs
-- have no category.
CREATE TABLE transfers (
  id              INTEGER PRIMARY KEY,
  from_account_id INTEGER NOT NULL REFERENCES accounts(id),
  to_account_id   INTEGER NOT NULL REFERENCES accounts(id),
  from_txn_id     INTEGER UNIQUE REFERENCES transactions(id) ON DELETE CASCADE,
  to_txn_id       INTEGER UNIQUE REFERENCES transactions(id) ON DELETE CASCADE,
  matched_by      TEXT NOT NULL CHECK (matched_by IN ('auto','rule','manual','import')),
  state           TEXT NOT NULL DEFAULT 'confirmed' CHECK (state IN ('suggested','confirmed')),
  confirmed_by    INTEGER REFERENCES users(id),
  mismatch_flag   INTEGER NOT NULL DEFAULT 0,    -- legs no longer equal and opposite; user must look
  created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  CHECK (from_account_id <> to_account_id),
  CHECK (from_txn_id IS NOT NULL OR to_txn_id IS NOT NULL)
) STRICT;

-- The bank's balance (in milestone 1, Actual's, written by the import) is the balance shown for
-- every account; the app's own sum only detects drift.
CREATE TABLE balance_snapshots (
  account_id    INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  as_of         TEXT NOT NULL,
  balance_minor INTEGER NOT NULL,
  source        TEXT NOT NULL CHECK (source IN ('provider','manual','import')),
  provider_balance_date TEXT,
  sync_run_id   INTEGER,                    -- REFERENCES sync_runs(id) ON DELETE SET NULL from milestone 2
  sign_corrected INTEGER NOT NULL DEFAULT 0,
  app_balance_posted_minor  INTEGER,        -- opening + posted transactions + adjustments, as of this snapshot
  app_balance_pending_minor INTEGER,        -- same plus pending; both NULL for balances-only accounts
  PRIMARY KEY (account_id, as_of)
) STRICT;

CREATE TABLE import_batches (
  id           INTEGER PRIMARY KEY,
  account_id   INTEGER REFERENCES accounts(id),  -- NULL only for format 'actual' (spans accounts)
  format       TEXT NOT NULL CHECK (format IN ('csv','ofx','actual')),  -- ofx covers OFX/QFX/QBO
  file_name    TEXT NOT NULL,
  mapping_id   INTEGER,                     -- REFERENCES import_mappings(id) ON DELETE SET NULL from milestone 2
  added        INTEGER NOT NULL,
  matched      INTEGER NOT NULL,
  skipped      INTEGER NOT NULL,
  imported_by  INTEGER REFERENCES users(id),
  imported_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  undone_at    TEXT,
  CHECK ((account_id IS NULL) = (format = 'actual'))
) STRICT;

CREATE TABLE import_batch_rows (
  batch_id       INTEGER NOT NULL REFERENCES import_batches(id) ON DELETE CASCADE,
  transaction_id INTEGER NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
  outcome        TEXT NOT NULL CHECK (outcome IN ('inserted','matched')),
  import_ref     TEXT NOT NULL,             -- OFX FITID, a hash of the CSV row's raw cells, or Actual's transaction id
  set_bank_category INTEGER NOT NULL DEFAULT 0,
  prior_fields   TEXT,                      -- 'actual' matched rows: JSON of category/categorized_by/memo/payee
                                            -- before the import filled them; restored on undo
  -- Actual import only: the row's Actual category name and what the mapping did with it.
  --   mapped:        took the mapped category
  --   undecided:     mapping said "Decide later" → uncategorized, listed in "From Actual, undecided"
  --   transfer:      mapping said "Transfer (not a category)" → uncategorized, transfer-like for detection (M3)
  --   uncategorized: had no category in Actual → runs the pipeline (M3)
  source_category    TEXT,
  source_disposition TEXT CHECK (source_disposition IN ('mapped','undecided','transfer','uncategorized')),
  PRIMARY KEY (batch_id, transaction_id)
) STRICT;
CREATE INDEX idx_import_ref ON import_batch_rows(import_ref);

-- Every amount that counts toward a category, splits expanded, excluded rows dropped.
-- Only budgeted accounts; transfer legs carry no category so they drop out.
-- Uncategorized rows appear with category_id NULL: reported as a temporary "Uncategorized" line so
-- totals never look better than reality. Legs of a Transfer candidate (state 'suggested') are left
-- out too.
CREATE VIEW v_category_lines AS
SELECT t.id AS txn_id, t.account_id, t.date, t.status, sp.category_id, sp.amount_minor
FROM transactions t
JOIN accounts a ON a.id = t.account_id AND a.role = 'budgeted'
JOIN transaction_splits sp ON sp.transaction_id = t.id
WHERE t.exclude_from_budget = 0
UNION ALL
SELECT t.id, t.account_id, t.date, t.status, t.category_id, t.amount_minor
FROM transactions t
JOIN accounts a ON a.id = t.account_id AND a.role = 'budgeted'
WHERE t.exclude_from_budget = 0
  AND t.category_id IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM transaction_splits sp WHERE sp.transaction_id = t.id)
UNION ALL
SELECT t.id, t.account_id, t.date, t.status, NULL, t.amount_minor
FROM transactions t
JOIN accounts a ON a.id = t.account_id AND a.role = 'budgeted'
WHERE t.exclude_from_budget = 0
  AND t.category_id IS NULL
  AND NOT EXISTS (SELECT 1 FROM transaction_splits sp WHERE sp.transaction_id = t.id)
  AND NOT EXISTS (SELECT 1 FROM transfers tr WHERE tr.from_txn_id = t.id OR tr.to_txn_id = t.id)
  AND t.awaiting_transfer_until IS NULL;
