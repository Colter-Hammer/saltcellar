-- Saltcellar v1: target schema (SQLite, STRICT, WAL).
-- Vocabulary: /GLOSSARY.md. Behaviour behind each table: docs/spec/.
-- This is the design the migrations build toward, milestone by milestone; it is not itself a migration.
-- Each table's comment names the milestone that first needs it (M1–M6).
--
-- Conventions
--   money:  INTEGER minor units of the household currency (cents for USD, none for JPY), never floats.
--           Columns end in _minor. The number of decimal places comes from the currency (household.currency_code).
--   sign:   from the account's point of view; money leaving is negative.
--   dates:  ISO 'YYYY-MM-DD' calendar dates, no timezone; months 'YYYY-MM'; timestamps UTC ISO-8601.
--           "Today", period boundaries and scheduled jobs use household.timezone.
--   bools:  INTEGER 0/1.
--   Rules marked "enforce in code" are invariants the Rust domain crate owns and tests.
--   Sections marked [sync] are reconciled by the bank sync part of the spec (milestone 2); treat them as
--   provisional until that lands.

PRAGMA foreign_keys = ON;

-- ───────────────────────── Household ─────────────────────────

CREATE TABLE household (                    -- M1. Exactly one row, written by the setup wizard.
  id            INTEGER PRIMARY KEY CHECK (id = 1),
  currency_code TEXT NOT NULL,              -- ISO 4217; fixed once any transaction exists (enforce in code)
  timezone      TEXT NOT NULL,              -- IANA name, pre-filled from the browser at setup, editable later
  setup_done_at TEXT                        -- NULL until the wizard finishes; the setup code stops working then
) STRICT;

-- ───────────────────────── Logins and auth ─────────────────────────

CREATE TABLE users (                        -- M1. A login. All equal: no admin role, no cap on the count.
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

CREATE TABLE sessions (                     -- M1. Built-in mode. 30-day sliding expiry; deleting rows = log out.
  token_hash   TEXT PRIMARY KEY,            -- hash of the cookie token, never the raw token
  user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  last_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  expires_at   TEXT NOT NULL                -- last_seen_at + 30 days, pushed forward on use
) STRICT;

CREATE TABLE login_failures (               -- M1. 5 failures in 15 min pause that username for 15 min.
  username   TEXT NOT NULL,                 -- as typed, so unknown names are throttled too
  failed_at  TEXT NOT NULL
) STRICT;
CREATE INDEX idx_login_failures ON login_failures(username, failed_at);

-- One-time links. Only the hash of the token is stored; the link is shown once.
CREATE TABLE invites (                      -- M6. Expires after 7 days.
  token_hash  TEXT PRIMARY KEY,
  created_by  INTEGER NOT NULL REFERENCES users(id),
  remote_user TEXT,                         -- trusted-header mode: the header value this invite accepts
  expires_at  TEXT NOT NULL,
  used_at     TEXT,
  used_by     INTEGER REFERENCES users(id)
) STRICT;

CREATE TABLE password_resets (              -- M1 (CLI-issued) / M6 (issued by another login in Settings).
  token_hash TEXT PRIMARY KEY,
  user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created_by INTEGER REFERENCES users(id),  -- NULL = issued by the `reset-password` CLI command
  expires_at TEXT NOT NULL,                 -- 24 hours
  used_at    TEXT
) STRICT;

-- ───────────────────────── Accounts and sync ─────────────────────────

CREATE TABLE connections (                  -- [sync] M2. One bank-data provider login (one SimpleFIN access URL).
  id               INTEGER PRIMARY KEY,
  provider         TEXT NOT NULL CHECK (provider IN ('simplefin')),
  label            TEXT NOT NULL,
  secret_encrypted BLOB NOT NULL,           -- AEAD-encrypted; key lives outside the DB
  last_sync_at     TEXT,
  last_error       TEXT,
  status           TEXT NOT NULL DEFAULT 'ok' CHECK (status IN ('ok','token_revoked','unpaid')),
                     -- token_revoked (403/gen.auth) or unpaid (402): all syncing paused until a new
                     -- setup token is claimed, which replaces secret_encrypted on this same row
  paused_at        TEXT
) STRICT;

CREATE TABLE accounts (                     -- M1
  id                  INTEGER PRIMARY KEY,
  name                TEXT NOT NULL UNIQUE,
  institution         TEXT,
  type                TEXT NOT NULL CHECK (type IN
                        ('checking','savings','credit_card','investment','loan','cash','other')),
  role                TEXT NOT NULL CHECK (role IN ('budgeted','tracking')),
                        -- budgeted: activity counts toward budgets (checking, shared cards, play-money accounts)
                        -- tracking: balance over time only; money moved in counts as Saved
  has_transactions    INTEGER NOT NULL DEFAULT 1, -- 0 = balances only (investments, loans): transfers to it are one-legged
  in_net_worth        INTEGER NOT NULL DEFAULT 1, -- for the post-v1 net worth report
  closed              INTEGER NOT NULL DEFAULT 0,
  sort_order          INTEGER NOT NULL DEFAULT 0,
  connection_id       INTEGER REFERENCES connections(id),
  external_account_id TEXT,                 -- provider account id. The Actual import sets it (with connection_id NULL)
                                            -- from Actual's own bank-sync id, so a later sync attaches the account
                                            -- as already tracked.
  -- Opening balance: derived, never a transaction. From the Actual import (M1): the account's Actual balance
  -- minus the imported rows, dated the day before the earliest imported row. On first sync (M2): first bank
  -- balance minus back-filled rows. When older history arrives, the date moves back and the amount is
  -- recomputed so drift is unchanged. NULL for balances-only accounts.
  opening_balance_minor INTEGER,
  opening_date          TEXT,
  balance_includes_pending INTEGER,         -- [sync] does the bank's balance count pending rows? NULL = unknown
  UNIQUE (connection_id, external_account_id),
  CHECK ((opening_balance_minor IS NULL) = (opening_date IS NULL))
) STRICT;

-- [sync] M2. Every account a connection reports. New ones wait as 'pending' until the user tracks
-- (creates/links an accounts row, choosing its role) or ignores them; ignored ones can be revived.
-- After a re-link at the Bridge or a new token, a new external id can be linked to an existing
-- account ("this is the same as…"): its account_id moves to the new row, history carries over.
CREATE TABLE connection_accounts (
  connection_id       INTEGER NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
  external_account_id TEXT NOT NULL,
  provider_name       TEXT NOT NULL,
  institution         TEXT,
  decision            TEXT NOT NULL DEFAULT 'pending' CHECK (decision IN ('pending','tracked','ignored')),
  account_id          INTEGER UNIQUE REFERENCES accounts(id),
  first_seen_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (connection_id, external_account_id),
  CHECK ((decision = 'tracked') = (account_id IS NOT NULL))
) STRICT;

-- M1 ('import' source) / [sync] M2 ('provider', 'manual').
-- The bank's balance is the balance shown for every account; the app's own sum only detects drift.
-- 'import': the Actual import writes one snapshot per account, its Actual balance as of the export's last
-- row, so balances match Actual from day one.
-- Provider snapshots: `balance` only (never `available-balance`), written only when `balance-date` moves
-- forward, at most one per account per UTC day (a later refresh that day replaces it). Kept forever.
-- Drift = balance_minor − app balance. Flagged when non-zero drift holds at the same value for 2 consecutive
-- snapshots; clears when it returns to zero. Never auto-adjusted.
-- Manual snapshots only on accounts with no working sync.
CREATE TABLE balance_snapshots (
  account_id    INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  as_of         TEXT NOT NULL,
  balance_minor INTEGER NOT NULL,
  source        TEXT NOT NULL CHECK (source IN ('provider','manual','import')),
  provider_balance_date TEXT,
  sync_run_id   INTEGER REFERENCES sync_runs(id) ON DELETE SET NULL,
  sign_corrected INTEGER NOT NULL DEFAULT 0,
  app_balance_posted_minor  INTEGER,        -- opening + posted transactions + adjustments, as of this snapshot
  app_balance_pending_minor INTEGER,        -- same plus pending; both NULL for balances-only accounts
  PRIMARY KEY (account_id, as_of)
) STRICT;

-- [sync] M2. "Accept $X": unexplained drift the user chose not to trace. Not a transaction: never in
-- spending, budgets, Review or transfer matching. Undo keeps the row so a drift report can count them.
CREATE TABLE balance_adjustments (
  id           INTEGER PRIMARY KEY,
  account_id   INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  date         TEXT NOT NULL,
  amount_minor INTEGER NOT NULL,
  note         TEXT,
  accepted_by  INTEGER REFERENCES users(id),
  accepted_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  undone_at    TEXT,
  undone_by    INTEGER REFERENCES users(id)
) STRICT;

-- [sync] M2. Kept 90 days, shown in Settings › Sync. One run at a time: a second request joins the running one.
CREATE TABLE sync_runs (
  id            INTEGER PRIMARY KEY,
  connection_id INTEGER NOT NULL REFERENCES connections(id),
  trigger       TEXT NOT NULL CHECK (trigger IN ('scheduled','manual','retry','catch_up','backfill')),
  started_at    TEXT NOT NULL,
  finished_at   TEXT,
  requests      INTEGER NOT NULL DEFAULT 0,  -- Bridge requests made; app cap is 12 per rolling 24h
  added         INTEGER NOT NULL DEFAULT 0,
  updated       INTEGER NOT NULL DEFAULT 0,
  error_code    TEXT,
  error         TEXT                         -- sanitized; never amounts, merchants or the access URL
) STRICT;

CREATE TABLE sync_account_results (         -- [sync] M2. Banks fail independently.
  sync_run_id          INTEGER NOT NULL REFERENCES sync_runs(id) ON DELETE CASCADE,
  account_id           INTEGER NOT NULL REFERENCES accounts(id),
  ok                   INTEGER NOT NULL,
  error_code           TEXT,
  error                TEXT,
  provider_balance_date TEXT,               -- > 2 days old = stale, > 5 = needs attention
  PRIMARY KEY (sync_run_id, account_id)
) STRICT;

CREATE TABLE sync_raw_responses (           -- [sync] M2. Only with the "keep raw responses (debug)" setting;
  sync_run_id INTEGER NOT NULL REFERENCES sync_runs(id) ON DELETE CASCADE, -- pruned after 7 days
  seq         INTEGER NOT NULL,
  received_at TEXT NOT NULL,
  body        BLOB NOT NULL,                -- encrypted; never logged
  PRIMARY KEY (sync_run_id, seq)
) STRICT;

-- ───────────────────────── Imports ─────────────────────────
-- [sync] M2 for files. Matching a file row to a stored row: same account, exact amount, date within ±3 days;
-- description only breaks ties; identical rows are counted, not collapsed; a tie with no tiebreaker →
-- "possible duplicate" in the preview. Nothing is written until the user commits the preview.

CREATE TABLE import_mappings (              -- [sync] M2. CSV column mapping, saved per account on first import
  id               INTEGER PRIMARY KEY,
  account_id       INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  header_signature TEXT NOT NULL,
  config           TEXT NOT NULL,           -- JSON: date col, post_date col?, description col, amount col OR
                                            --   debit/credit cols, flip_sign, date_format, category col?, memo col?,
                                            --   skip_rows
  updated_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  UNIQUE (account_id, header_signature)
) STRICT;

-- M1 ('actual') / [sync] M2 ('csv', 'ofx'). One committed import, undoable as a whole from Settings › Imports:
-- deletes the rows it inserted (asking first if any were edited since) and restores fields it filled on
-- matched rows. Never deletes rows that sync has since adopted (source = 'provider').
-- The Import from Actual wizard is one batch for the whole export (account_id NULL).
CREATE TABLE import_batches (
  id           INTEGER PRIMARY KEY,
  account_id   INTEGER REFERENCES accounts(id),  -- NULL only for format 'actual' (spans accounts)
  format       TEXT NOT NULL CHECK (format IN ('csv','ofx','actual')),  -- ofx covers OFX/QFX/QBO
  file_name    TEXT NOT NULL,
  mapping_id   INTEGER REFERENCES import_mappings(id) ON DELETE SET NULL,
  added        INTEGER NOT NULL,
  matched      INTEGER NOT NULL,
  skipped      INTEGER NOT NULL,
  imported_by  INTEGER REFERENCES users(id),
  imported_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  undone_at    TEXT,
  CHECK ((account_id IS NULL) = (format = 'actual'))
) STRICT;

CREATE TABLE import_batch_rows (            -- M1
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

-- ───────────────────────── Categories, payees, tags ─────────────────────────

CREATE TABLE category_groups (              -- M1. Container only; never holds transactions.
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL UNIQUE,
  kind       TEXT NOT NULL DEFAULT 'expense' CHECK (kind IN ('expense','income')),
  sort_order INTEGER NOT NULL DEFAULT 0,
  hidden     INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE TABLE categories (                   -- M1
  id         INTEGER PRIMARY KEY,
  group_id   INTEGER NOT NULL REFERENCES category_groups(id),
  name       TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  hidden     INTEGER NOT NULL DEFAULT 0,    -- hide instead of delete once transactions use it
  UNIQUE (group_id, name)
) STRICT;

CREATE TABLE payees (                       -- M1. The merchant/counterparty as the household thinks of it.
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL UNIQUE,
  review_mode TEXT NOT NULL DEFAULT 'auto'
                CHECK (review_mode IN ('auto','always','never'))
                -- auto:   normal cascade; mixed or often-split history → Review
                -- always: never auto-categorized; Review with suggestions (starter set: docs/spec/m1)
                -- never:  apply the top history pick even if mixed; only a first sighting goes to Review
) STRICT;

CREATE TABLE payee_aliases (                -- M1. Normalized bank text → payee; merging payees = repointing aliases.
  normalized_text TEXT PRIMARY KEY,
  payee_id        INTEGER NOT NULL REFERENCES payees(id) ON DELETE CASCADE
) STRICT;

CREATE TABLE tags (                         -- M1
  id   INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE
) STRICT;

-- ───────────────────────── Transactions ─────────────────────────
-- [sync] Manual entry (M2): a manual row needs a category at entry (suggestions only), so it never reaches
-- Review; shows a "manual" badge until sync adopts it (same account, exact amount, bank date −3/+14 days).

CREATE TABLE transactions (                 -- M1
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
                        -- The classifier never sets a category in v1 (suggestions only), so it has no value here.
  categorization_reason TEXT,               -- shown on the review card and the detail panel:
                                            -- "rule: <name>", "9 of 10 → Groceries", "always-review payee", "mixed history"
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

CREATE TABLE dropped_transactions (         -- [sync] M2. Pending rows the bank dropped; kept 30 days.
  id           INTEGER PRIMARY KEY,
  account_id   INTEGER NOT NULL REFERENCES accounts(id),
  snapshot     TEXT NOT NULL,
  user_edited  INTEGER NOT NULL,
  dropped_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
) STRICT;

CREATE TABLE deleted_bank_rows (            -- [sync] M2. Sync skips these ids so the daily window doesn't bring them back.
  account_id   INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  external_id  TEXT NOT NULL,
  deleted_by   INTEGER REFERENCES users(id),
  deleted_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (account_id, external_id)
) STRICT;

CREATE TABLE transaction_splits (           -- M1. Enforce in code: lines sum to the parent amount.
  id             INTEGER PRIMARY KEY,
  transaction_id INTEGER NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
  category_id    INTEGER NOT NULL REFERENCES categories(id),
  amount_minor   INTEGER NOT NULL,
  memo           TEXT
) STRICT;

CREATE TABLE transaction_tags (             -- M1
  transaction_id INTEGER NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
  tag_id         INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (transaction_id, tag_id)
) STRICT;

-- M1 (Actual's linked transfers, matched_by 'import') / M3 (detection, Transfers screen).
-- A move between our own accounts. Two legs normally; one leg when the other account has no transaction
-- data (has_transactions = 0). Enforce in code: legs are in the named accounts, from-leg = −(to-leg), legs
-- have no category. Strong pairs → 'confirmed' + matched_by 'auto'; weak → 'suggested' (a Transfer candidate:
-- Transfers screen, legs out of Review and Uncategorized until decided; rejecting deletes the row).
-- [sync] Matching windows and strength rules: see the milestone 3 transfer section.
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

-- ───────────────────────── Categorization (M3) ─────────────────────────

CREATE TABLE rules (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  priority   INTEGER NOT NULL DEFAULT 100,  -- tie-break only; run order = specificity
  enabled    INTEGER NOT NULL DEFAULT 1,
  conditions TEXT NOT NULL,                 -- JSON array, ALL must match. Each: {"field": ..., "op": ..., "value": ...}
                                            --   payee_id: is | raw_description: contains, matches (regex, case-insensitive)
                                            --   amount_minor: is, between, sign (in/out) | account_id: is
                                            --   bank_category: is, contains (case-insensitive)
  actions    TEXT NOT NULL,                 -- JSON array. Each one of: {"set_category": id}, {"rename_payee": id},
                                            --   {"send_to_review": true}, {"add_tag": id}, {"mark_transfer_to": account_id}
                                            -- No split actions in v1.
  created_by INTEGER REFERENCES users(id),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
) STRICT;

-- A rule the app proposes; never applied until a user accepts it (one tap, pre-filled, editable).
CREATE TABLE rule_suggestions (
  id          INTEGER PRIMARY KEY,
  trigger     TEXT NOT NULL CHECK (trigger IN ('repeated_correction','amount_pattern','consistent_review_pick',
                                               'recheck_keep','actual_import','one_legged_transfer')),
                                            -- one_legged_transfer [sync]: a description names a balances-only
                                            -- account (last-4 or institution) → mark_transfer_to
  payee_id    INTEGER REFERENCES payees(id) ON DELETE CASCADE,
  conditions  TEXT NOT NULL,                -- same JSON shape as rules
  actions     TEXT NOT NULL,
  status      TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','accepted','dismissed')),
  rule_id     INTEGER REFERENCES rules(id) ON DELETE SET NULL, -- set when accepted
  created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
) STRICT;

-- Every category change after the row first lands; feeds payee history, rule suggestions and accuracy stats.
CREATE TABLE categorization_feedback (
  id                    INTEGER PRIMARY KEY,
  transaction_id        INTEGER NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
  previous_category_id  INTEGER REFERENCES categories(id),
  previous_by           TEXT,               -- categorized_by before the change
  final_category_id     INTEGER REFERENCES categories(id),
  top_suggestion_id     INTEGER REFERENCES categories(id), -- Review only: the first suggestion shown
  user_id               INTEGER REFERENCES users(id),
  created_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
) STRICT;

-- History re-check: a row the user chose to Keep is never flagged again.
CREATE TABLE recheck_keeps (
  transaction_id INTEGER PRIMARY KEY REFERENCES transactions(id) ON DELETE CASCADE,
  kept_by        INTEGER REFERENCES users(id),
  kept_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
) STRICT;

-- ───────────────────────── Budgets (M4) ─────────────────────────

-- A target for a group (overall cap) or a category (limit inside its group's cap).
-- A multi-month category budget is judged against its own window only; it never counts toward a monthly group cap.
-- Windows are fixed: period_months long, starting at anchor_month (e.g. 3 months from 2027-02 → Feb–Apr, May–Jul…).
-- repeats = 0 → a one-time budget with exactly one window.
-- Enforce in code: warn when monthly category targets in a group exceed the group's target.
CREATE TABLE budgets (
  id            INTEGER PRIMARY KEY,
  category_id   INTEGER REFERENCES categories(id),
  group_id      INTEGER REFERENCES category_groups(id),
  period_months INTEGER NOT NULL DEFAULT 1 CHECK (period_months >= 1),
  anchor_month  TEXT NOT NULL,              -- 'YYYY-MM', first window's start
  repeats       INTEGER NOT NULL DEFAULT 1,
  archived      INTEGER NOT NULL DEFAULT 0,
  CHECK ((category_id IS NULL) <> (group_id IS NULL))
) STRICT;

-- Target history: a change applies from that window forward; earlier windows keep old targets.
CREATE TABLE budget_targets (
  budget_id      INTEGER NOT NULL REFERENCES budgets(id) ON DELETE CASCADE,
  effective_from TEXT NOT NULL,             -- window start 'YYYY-MM'
  target_minor   INTEGER NOT NULL,
  PRIMARY KEY (budget_id, effective_from)
) STRICT;

-- One-off override for a single window ("December: Gifts $600").
CREATE TABLE budget_overrides (
  budget_id    INTEGER NOT NULL REFERENCES budgets(id) ON DELETE CASCADE,
  window_start TEXT NOT NULL,
  target_minor INTEGER NOT NULL,
  PRIMARY KEY (budget_id, window_start)
) STRICT;

-- ───────────────────────── Funds (M5) ─────────────────────────

-- A running balance that carries forward: contributions in, spending in its category out.
-- play_money: fixed monthly contribution, owned by one login.
-- sinking:    contribution = its budget's target spread over the window.
-- Balance = opening + contributions + adjustments − spending in category_id since start_month. Always computed.
-- When account_id is set (must be a budgeted account; enforce in code), contributions are the actual
-- transfers into that account instead of the schedule; spending is still the category on any account; the
-- account's real balance is shown alongside (gap = money still to move, e.g. to pay a card).
CREATE TABLE funds (
  id                    INTEGER PRIMARY KEY,
  name                  TEXT NOT NULL UNIQUE,
  kind                  TEXT NOT NULL CHECK (kind IN ('play_money','sinking')),
  owner_user_id         INTEGER REFERENCES users(id),     -- play money only
  category_id           INTEGER NOT NULL UNIQUE REFERENCES categories(id), -- spending that draws it down
  budget_id             INTEGER UNIQUE REFERENCES budgets(id),             -- sinking only
  account_id            INTEGER REFERENCES accounts(id),  -- optional real account backing
  start_month           TEXT NOT NULL,
  opening_balance_minor INTEGER NOT NULL DEFAULT 0,
  archived              INTEGER NOT NULL DEFAULT 0,
  CHECK ((kind = 'sinking') = (budget_id IS NOT NULL)),
  CHECK ((kind = 'play_money') = (owner_user_id IS NOT NULL))
) STRICT;

CREATE TABLE fund_contributions (           -- play money monthly amount, effective from a month forward
  fund_id        INTEGER NOT NULL REFERENCES funds(id) ON DELETE CASCADE,
  effective_from TEXT NOT NULL,
  monthly_minor  INTEGER NOT NULL,
  PRIMARY KEY (fund_id, effective_from)
) STRICT;

CREATE TABLE fund_adjustments (             -- manual corrections to a paper fund
  id           INTEGER PRIMARY KEY,
  fund_id      INTEGER NOT NULL REFERENCES funds(id) ON DELETE CASCADE,
  date         TEXT NOT NULL,
  amount_minor INTEGER NOT NULL,
  note         TEXT,
  created_by   INTEGER REFERENCES users(id)
) STRICT;

-- ───────────────────────── Savings goals (M5) ─────────────────────────

-- contribution: move target_minor into the goal's accounts per period_months (under = bad).
-- balance:      hold target_minor across the goal's accounts; by target_date, or ongoing
--               (target_date NULL → met while balance >= target, unmet again after a withdrawal).
CREATE TABLE savings_goals (
  id            INTEGER PRIMARY KEY,
  name          TEXT NOT NULL UNIQUE,
  kind          TEXT NOT NULL CHECK (kind IN ('contribution','balance')),
  target_minor  INTEGER NOT NULL,
  period_months INTEGER,                    -- contribution only
  target_date   TEXT,                       -- balance only; NULL = ongoing
  archived      INTEGER NOT NULL DEFAULT 0,
  CHECK ((kind = 'contribution') = (period_months IS NOT NULL)),
  CHECK (kind = 'balance' OR target_date IS NULL)
) STRICT;

CREATE TABLE savings_goal_accounts (        -- enforce in code: tracking accounts only
  goal_id    INTEGER NOT NULL REFERENCES savings_goals(id) ON DELETE CASCADE,
  account_id INTEGER NOT NULL REFERENCES accounts(id),
  PRIMARY KEY (goal_id, account_id)
) STRICT;

-- ───────────────────────── Settings, jobs, notifications ─────────────────────────

-- In-app Settings (everything that isn't needed before login or doesn't touch the host). Keys, all JSON values:
--   backups.enabled (default true), backups.age_public_key, backups.keep_daily (30), backups.keep_monthly (12)
--   ntfy.server_url, ntfy.access_token (M6; ntfy is off until configured)
--   restore_drill.last_at (M6)
--   sync.* [sync] (connection settings, sync time, keep raw responses)
CREATE TABLE settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;

CREATE TABLE backup_runs (                  -- M6. Nightly; a failed or missing run raises an alert.
  id          INTEGER PRIMARY KEY,
  trigger     TEXT NOT NULL CHECK (trigger IN ('scheduled','download')),
  started_at  TEXT NOT NULL,
  finished_at TEXT,
  ok          INTEGER,
  file_name   TEXT,
  encrypted   INTEGER,
  error       TEXT
) STRICT;

CREATE TABLE user_reminder_prefs (          -- M6. Per-login review reminder.
  user_id          INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
  enabled          INTEGER NOT NULL DEFAULT 1,
  frequency        TEXT NOT NULL DEFAULT 'weekly' CHECK (frequency IN ('weekly','biweekly','monthly')),
  weekday          INTEGER NOT NULL DEFAULT 0,       -- 0 = Sunday
  time_local       TEXT NOT NULL DEFAULT '19:00',    -- household timezone
  nudge_threshold  INTEGER NOT NULL DEFAULT 25,      -- off-schedule push when the inbox exceeds this
  ntfy_topic       TEXT,                             -- per-login topic; NULL = in-app only
  last_sent_at     TEXT,
  last_nudge_at    TEXT                              -- at most one nudge between scheduled reminders
) STRICT;

-- ───────────────────────── Reporting views ─────────────────────────

-- M1. Every amount that counts toward a category, splits expanded, excluded rows dropped.
-- Only budgeted accounts; transfer legs carry no category so they drop out.
-- Uncategorized rows appear with category_id NULL: reported as a temporary "Uncategorized" line so totals
-- never look better than reality. Legs of a Transfer candidate (state 'suggested') are left out too.
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

-- M3. The Review inbox: uncategorized, unsplit rows on budgeted accounts that aren't transfer legs (confirmed or
-- candidate), aren't waiting for a transfer partner, aren't review_quiet, and aren't Actual "Decide later" rows.
-- Ordered oldest first by the API.
CREATE VIEW v_review_inbox AS
SELECT t.*
FROM transactions t
JOIN accounts a ON a.id = t.account_id AND a.role = 'budgeted'
WHERE t.category_id IS NULL
  AND t.exclude_from_budget = 0
  AND t.review_quiet = 0
  AND t.awaiting_transfer_until IS NULL
  AND NOT EXISTS (SELECT 1 FROM transaction_splits sp WHERE sp.transaction_id = t.id)
  AND NOT EXISTS (SELECT 1 FROM transfers tr WHERE tr.from_txn_id = t.id OR tr.to_txn_id = t.id)
  AND NOT EXISTS (SELECT 1 FROM import_batch_rows ib JOIN import_batches b ON b.id = ib.batch_id
                  WHERE ib.transaction_id = t.id AND b.undone_at IS NULL
                    AND ib.source_disposition = 'undecided');

-- M4. Cash flow "Saved": net money moved from budgeted into tracking accounts (negative = withdrawn).
CREATE VIEW v_saved_lines AS
SELECT tr.id AS transfer_id,
       COALESCE(f.date, t2.date) AS date,
       CASE WHEN fa.role = 'budgeted' AND ta.role = 'tracking' THEN  ABS(COALESCE(f.amount_minor, t2.amount_minor))
            WHEN fa.role = 'tracking' AND ta.role = 'budgeted' THEN -ABS(COALESCE(f.amount_minor, t2.amount_minor))
       END AS saved_minor
FROM transfers tr
JOIN accounts fa ON fa.id = tr.from_account_id
JOIN accounts ta ON ta.id = tr.to_account_id
LEFT JOIN transactions f  ON f.id  = tr.from_txn_id
LEFT JOIN transactions t2 ON t2.id = tr.to_txn_id
WHERE fa.role <> ta.role
  AND tr.state = 'confirmed';
