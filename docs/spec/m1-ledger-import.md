# Milestone 1: Ledger + Import from Actual

**Goal:** the app runs locally, has a household and a login, and shows the owner's full history from their old budgeting tool (Actual Budget) correctly: every account, every transaction, every balance.

**Done when:** the owner imports their real Actual export on their PC and every account's balance matches Actual, with row counts matching; the Playwright smoke test (setup → log in → import the fake export → see transactions and balances) passes in CI; every check in [Foundations › Coding standards](foundations.md#coding-standards) is green and required.

**Not in this milestone:** bank sync, CSV/QFX files, manual entry (milestone 2); categorization, Review, editing categories on transactions, transfer detection, History re-check (milestone 3); Overview, budgets (milestone 4). No Overview: the importer is reached from Accounts and More.

## Order of work

1. **Repo, guards, checks and standards docs first**, before any feature code: the private GitHub repo with `main` protected, ignores, `lefthook` hooks, the leak guard and `gitleaks`, the CI workflow running every check, `docs/standards.md`, `CONTRIBUTING.md`, `justfile` (`just check`), `.editorconfig`, Renovate, PR-title check, migration-immutability check. A deliberately bad commit (a denylisted word, a `.sqlite` file) must be blocked locally and in CI.
2. **Skeleton:** Cargo workspace with the layered crates, migrations runner, axum serving the built Vue app and `/api/v1/health`, OpenAPI generation wired into the frontend build, the Vue shell with tab bar → sidebar, `Dockerfile`, `compose.yaml`, first `docs/deploy.md`.
3. **Setup wizard and login.**
4. **Categories.**
5. **Import from Actual**, with Accounts and Transactions screens to see the result, and the "From Actual, undecided" list.

Each step after the first is a thin vertical slice that ends in something checkable in a browser.

## Setup wizard

Shown on any request while `household.setup_done_at` is NULL; nothing else is reachable.

1. **Setup code.** "Enter the setup code from the app's log." The code is printed to the container log at startup (see [Foundations › Auth](foundations.md#auth)).
2. **Currency and timezone.** Currency: searchable ISO 4217 picker, default from the browser locale *(agent)*. Timezone: IANA picker pre-filled from the browser.
3. **First login.** Display name, username, password (≥ 12 characters, typed twice).
4. **Done.** Logs the new login in and lands on Transactions, whose empty state offers **Import from Actual** and **Set up categories** *(agent)*.

The full **Get started** checklist on Overview arrives with Overview in milestone 4.

## Login and profile

- Log-in page (username, password), with the throttle message "Too many attempts. Try again in N minutes." Log out from More.
- `budget reset-password <login>` CLI subcommand prints a one-time link valid for 24 hours *(agent)*; the link opens a set-new-password page and logs the person in.
- **Settings › Profile** *(agent)*: display name, display locale, week start, change password, log out everywhere.

## Categories

**Settings › Categories** (also under More). Groups (expense or income) containing categories; every category belongs to exactly one group; groups never hold transactions.

- Add, rename, reorder (drag on desktop, move up/down on phone) and hide groups and categories. A category can be deleted only while no transaction, split line or rule uses it; otherwise it can be hidden *(agent)*.
- When there are no categories, the screen offers **Use starter categories** or **Start empty**. The Actual importer's **+ New category…** also creates categories.

**Starter categories** (generic, deliberately no catch-all "General"; unclear charges go to Review):

| Group | Categories |
|---|---|
| Housing | Mortgage & Rent *(agent: "& Rent" for portability)*, Utilities, Internet & Phone, Home Maintenance |
| Food | Groceries, Restaurants, Fast Food & Coffee |
| Transportation | Gas, Car Maintenance, Car Purchase |
| Health & Fitness | Medical, Dental & Vision, Fitness |
| Insurance | Insurance |
| Kids | Activities, School & Childcare |
| Shopping | Household, Clothing, Personal Care |
| Fun | Entertainment, Travel, Subscriptions |
| Giving | Tithing & Donations, Gifts |
| Pets | Pet Care |
| Financial | Bank Fees & Interest, Taxes |
| Income *(income group)* | Paycheck, Interest, Tax Refunds, Other Income |

No Play Money group: a play-money category is created when a person sets up their play-money fund (milestone 5). Kids is included but easy to delete.

The starter set also seeds these payees as **always review** (merchants whose charges can be anything): Amazon, Target, Walmart, Costco, Sam's Club, Venmo, Cash Withdrawal (ATM and teller withdrawals), Check (any numbered check), eBay. Matching bank text to these payees is the normalizer's job in milestone 3; milestone 1 just creates the rows.

## Accounts

Read-only in this milestone; accounts are created by the Actual importer. Every account has a **role**: *budgeted* (activity counts toward budgets) or *tracking* (balance over time only; money moved in counts as Saved), and a *type* (checking, savings, credit card, investment, loan, cash, other). Balances-only accounts (`has_transactions = 0`) hold balance snapshots and no transactions.

- **Phone:** list grouped Budgeted / Tracking / Closed, each row with name, type and balance. **Desktop:** a table with name, institution, type, role, balance, last transaction date.
- **Account detail:** balance, opening balance and its date, row count, a link to Transactions filtered to the account.
- **Balance shown** is the latest balance snapshot. In this milestone that's the one the import wrote, so it equals Actual's balance. The app's own balance (opening + transactions) is computed beside it; if they ever differ that's drift, which milestone 2 surfaces.
- Empty state: "No accounts yet" with **Import from Actual**.

## Transactions

Read-only, except setting categories on undecided rows (below).

- **Phone:** list grouped by date (newest first), each row with payee, category (or "Uncategorized"), account and amount. Tapping opens a detail sheet.
- **Desktop:** a table (date, payee, category, account, amount, tags) with a detail panel beside it.
- **Search** across payee, raw description, memo and amount (typing `12.50` matches the amount).
- **Filters** (chips): account, category (including Uncategorized), date range, tag, **From Actual, undecided**, **Imported, uncategorized** (from milestone 3).
- **Detail:** date, amount, account, payee, raw description, category or split lines, memo, tags, status, source (import / bank / manual), how it got its category (`categorization_reason`), and the import batch it came from.
- Transfer legs show "Transfer to/from <account>" in place of a category.

## Import from Actual

A one-time wizard, reached from More › Import from Actual, the Accounts empty state and Transactions' empty state: **file → accounts → categories → done**. Nothing is written until the last step commits, and the whole import is one **Import batch** that can be undone.

### Step 1: File

Upload the Actual export `.zip` (limit 100 MB *(agent)*). The server accepts `db.sqlite` + `metadata.json` at the zip root or both inside exactly one shared folder, opens `db.sqlite` read-only from a temporary file, and shows the budget name and counts (accounts, transactions, categories, date range). Refused with a clear message: not an Actual export; the household currency doesn't use 2 decimal places (Actual stores cents) *(agent)*; an earlier Actual import that hasn't been undone ("Undo the earlier import first", until milestone 2 makes re-import overlap-aware) *(agent)*.

### Step 2: Accounts

One row per live Actual account: Actual name, closed or open, row count, Actual balance. For each, choose:

- **Create new** (default): name (pre-filled), type (best guess from the name, e.g. "card" → credit card *(agent)*), role (Actual's off-budget → tracking, otherwise budgeted), balances-only off. Closed accounts are created closed.
- **Same as** an existing app account (pre-selected when an app account has the same external account id; otherwise none) *(agent: only useful after milestone 2, when accounts can exist before the import)*.
- **Skip**: none of its rows are imported.

Accounts created here keep Actual's bank-sync account id (Actual's `accounts.account_id`, when set) as `external_account_id` with no connection, so milestone 2's first sync attaches them as already tracked.

Details *(agent)*: accounts are listed open before closed, on budget before off budget, then in Actual's order. The row count counts a split once and includes starting balances. A name that says nothing about the type gives checking on budget and other off budget. A new account's name must differ, ignoring case, from every other account's; Next waits until it does. A balances-only account brings its balance but none of its rows, so like a skipped one its rows don't count in step 3. The choices of steps 2 and 3 live in the browser until step 4 commits; Back keeps them.

### Step 3: Categories

One row per live Actual category that any imported row uses, with its group, row count and a **Goes to** picker. Desktop is a table; phone is stacked rows. Many Actual categories may go to one app category.

The picker offers every app category, **Transfer (not a category)** (for card payments and savings moves that Actual recorded as categories), **+ New category…** (name + group, created on the spot) and **Decide later**. Pre-fill: an app category with the same name (case-insensitive), otherwise Decide later. Import is never blocked: an unchosen row means Decide later. Actual's "Starting Balances" category isn't listed; those rows become opening balances.

Details *(agent)*: a split's lines count one row each in their own categories; starting-balance rows count in none. A live Actual category in a deleted group counts as deleted. Hidden app categories and groups aren't offered or pre-filled. When two app categories have the matching name, the one in a group named like the Actual group wins. **+ New category…** starts with the Actual category's name, in the app group named like the Actual group, else the first group of the same kind; it can also make a new group (the start when there is none), named after the Actual group, of its kind.

### Step 4: Done

Commits in one database transaction, then shows a summary: accounts created, rows imported per account, rows mapped / undecided / marked transfer / uncategorized, linked transfers, and "Actual's rules weren't copied, since payee history covers them." *(The "N rules suggested" part and the History re-check result join this step in milestone 3.)* Buttons: **See transactions**, **Decide on undecided rows** (if any).

### Reading the export

Read raw tables, not Actual's views (views can be missing or stale in older exports) *(agent)*. Check columns with `PRAGMA table_info` and default any that an older export lacks.

- **Tombstones:** skip every row with `tombstone = 1` in `accounts`, `categories`, `category_groups`, `payees`, `transactions`. A split child is alive only if it and its parent both are.
- **Id indirection:** `transactions.category` resolves through `category_mapping(id → transferId)`; `transactions.description` is a **payee id** resolving through `payee_mapping(id → targetId)`. A category that resolves to a tombstoned row means uncategorized.
- **Formats:** `amount` is signed integer cents (negative = outflow; NULL = 0); `date` is an integer `YYYYMMDD`.
- **Bank text** is `imported_description`; the bank's transaction id is `financial_id`; notes are `notes`.
- **Transfers:** a payee with `transfer_acct` set is an account, not a merchant. Twin rows link through `transferred_id`.
- **Splits:** parent row (`isParent = 1`, category NULL, amount = total) plus child rows (`isChild = 1`, `parent_id`). Never sum both.
- **Starting balances:** rows with `starting_balance_flag = 1`.
- Budgets (`reflect_budgets` / `zero_budgets`), rules, schedules and the CRDT message tables are not imported. *(agent: budgets aren't carried over, because the category set usually changes during import; budgets are set up fresh in milestone 4.)*

### Turning Actual rows into transactions

| Actual | App |
|---|---|
| alive, non-split, non-starting-balance row in an imported account | one `transactions` row, `source = 'import'`, `status = 'posted'` (Actual has no pending rows) |
| `date`, `amount` | ISO date, `amount_minor` |
| `imported_description` (or the payee name when NULL) | `raw_description` |
| `financial_id` | `external_id` |
| resolved payee (not a transfer payee) | `payee_id`, creating the payee by exact name if new; the normalized `raw_description` becomes a payee alias if not already taken *(agent: minimal normalization in milestone 1: lowercase, collapse whitespace, strip punctuation; the full normalizer arrives in milestone 3)* |
| `notes` | `memo`; each `#word` in the notes also becomes a tag *(agent)* |
| category mapped to an app category | `category_id`, `categorized_by = 'import'`, reason "From Actual: <name>" |
| category mapped to Decide later | uncategorized; listed in **From Actual, undecided**; kept out of Review |
| category mapped to Transfer | uncategorized; remembered as transfer-like so milestone 3's transfer detection pairs it or makes it a Transfer candidate |
| no category in Actual | uncategorized; waits for milestone 3's pipeline (below) |
| split parent + children | one row with `transaction_splits` lines in the mapped categories. If any child's category isn't mapped to an app category, the row comes in unsplit and uncategorized, with the Actual split written into the memo *(agent)* |
| linked transfer pair, both accounts imported | both legs, no category, one `transfers` row: `matched_by = 'import'`, `state = 'confirmed'` |
| linked transfer whose other account was skipped | the imported leg only, treated like a Transfer-mapped row |
| starting-balance row | not imported; feeds the opening balance |

Every row's Actual category name and what the mapping did with it are stored on its `import_batch_rows` entry (`source_category`, `source_disposition`), with Actual's transaction id as `import_ref`. `dedupe_key` uses the shared dedupe function defined with the bank sync sections; until those land, a hash of account, date, amount and normalized description *(agent)*.

**Balances.** For each imported account, the import writes a balance snapshot (`source = 'import'`) equal to the account's Actual balance (sum of all its alive rows, starting balances included) as of its last row's date, and sets the **opening balance** to that balance minus the imported rows, dated the day before the earliest imported row. Never a transaction, never income. An account with no imported rows gets its Actual balance as the opening balance, dated the import day *(agent)*.

**Rows uncategorized in Actual** stay uncategorized in this milestone (the Transactions "Uncategorized" filter shows them). When milestone 3 ships, its pipeline runs once over them: leftovers dated within 90 days of the import date go to Review; older ones are marked quiet and listed under **Imported, uncategorized** instead.

## From Actual, undecided

Under More, shown while any undecided rows remain. Rows grouped by their Actual category, each group showing count and total with **Move all to…** (a category picker). A group expands to its rows; tapping one opens the transaction detail with a category picker. Setting a category here records `categorized_by = 'user'`. These rows stay out of Review.

## Settings › Imports

Lists import batches (date, format, file, who, rows added / matched). **Undo** on an active batch:

- Asks first if any of its rows were edited since.
- Deletes the rows it inserted (and their splits, tags and transfers), restores fields it filled on matched rows, and marks the batch undone.
- Deletes accounts the batch created if they have no rows left, and payees left with no transactions *(agent)*. Categories created with + New category stay.
- Never deletes rows that bank sync has since taken over (milestone 2).
