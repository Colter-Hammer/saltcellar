# Milestone 2: Bank data in

**Goal:** bank data arrives on its own every day, files fill the gaps, and anything can be entered by hand. The bank's balance is shown for every account, and any gap between it and the app's own rows is visible and traceable.

**Done when:** on the owner's PC, a real SimpleFIN connection syncs every tracked account daily without help, balances match the banks, and the Actual history and the synced rows sit side by side with no duplicates; a CSV and an OFX file import into an account; a manual row is adopted when the bank sends it; the Playwright smoke test (connect to a fake SimpleFIN server → track accounts → sync → import a file → enter a manual row → see it adopted) passes in CI on fake data.

**Not in this milestone:** the categorization pipeline, Review and transfer detection (milestone 3). Rows that arrive here stay uncategorized until milestone 3 ships and its pipeline runs once over them; a person can already set categories by hand *(agent)*.

Provider: SimpleFIN Bridge ([ADR-0010](../adr/0010-simplefin-provider.md)). Terms are from the [glossary](../../GLOSSARY.md): connection, bank link, pending transaction, balance snapshot, drift, balance adjustment, opening balance, manual account, import batch, bank category, adopted row, stale, needs attention.

## Order of work *(agent)*

1. **Accounts become editable**, manual accounts and manual entry. Needs no provider, so it lands first and is checkable at once.
2. **Fake SimpleFIN server** in `fixtures/` (claim + `/accounts` with scripted scenarios: pending rows posting, an id re-issued, a bank needing re-auth, a revoked token, an account missing from a response). Every sync rule below is tested against it, never against the real Bridge.
3. **Connection:** claim a setup token, list the connection's accounts, track or ignore them.
4. **Sync engine:** the request, reading the feed, matching rows, pending rows, back-fill.
5. **Schedule and failures:** daily run, Sync now, the request budget, retries, catch-up, run history.
6. **Balances:** snapshots, opening balances, drift, balance adjustments.
7. **Sync health in the UI.**
8. **CSV and OFX/QFX import.**
9. **Import from Actual becomes overlap-aware.**

## Accounts

Accounts become editable (they were read-only in milestone 1): name, institution, type, role (budgeted / tracking), balances-only, in net worth, closed, order. Changing role or balances-only on an account with rows asks first, since it changes what counts toward budgets *(agent)*.

**New account** (Accounts, + button): a **manual account** (name, type, role) that asks "What's in it today?"; the answer is its first manual balance snapshot and its opening balance is derived from it. Synced accounts aren't created here; they arrive through the connection. The + opens the form at once, since it makes nothing else; Cash is the type chosen first *(agent)*. The opening balance is the answer itself, dated the day before today in the household's timezone *(agent)*. The new account goes last in the order *(agent)*.

**New accounts prompt.** Every account the connection reports is recorded (`connection_accounts`). One that isn't linked yet waits at the top of Accounts as "N new accounts found", each with **Track · budgeted**, **Track · tracking** or **Ignore**. Nothing from it counts until decided. Tracking one creates its account (type guessed from the name, balances-only chosen at that moment *(agent)*), or links it to an existing one with **This is the same as…**, which moves the existing account (its history and opening balance) onto the new feed account. Ignored accounts are listed under **Ignored** and can be revived with **Start tracking…**.

- An account the Actual import created with a provider account id is attached on first sight, already tracked with the role chosen in the import; it skips the prompt.
- A bank re-link or a new setup token can bring new account ids. The new ids arrive in the prompt with **This is the same as…** pre-filled when the name and institution match a tracked account whose old id has stopped arriving *(agent)*.
- A manual account that later appears through the connection is linked with **This is the same as…**: the back-fill adopts its rows (below), unmatched manual rows inside the back-fill window get the `not_seen_at_bank` flag, older manual rows stay, and manual balance entry stops.

Details of the prompt *(agent)*:

- **When it fills:** right after a claim (or a replaced token), one request lists the connection's accounts (`start-date` 45 days back, the first half of the back-fill); a sync fills it from then on. If that request fails the claim still stands: a 403 or 402 pauses the connection as a sync would, anything else is logged.
- **Tracking** opens a small form: the role (the button pressed chooses it first; **Start tracking…** starts at budgeted), **Balances only** (ticked when the name says investment or loan), and **This is the same as…** (a new account, or any open account). A new account takes the bank's name, made unique with a number (`Demo Savings 2`), and its bank as the institution; it has no opening balance until the back-fill. Linking keeps the existing account's name and history and takes the role and balances-only chosen; the feed account it had, if any, becomes ignored.
- **Another currency:** the account is recorded, listed under **Can't be tracked** with why ("… reports its money in CAD, and this household keeps EUR, so it can't be tracked."), and tracking it is refused (`foreign_currency`).
- **Re-attaching:** an account with the provider id and no feed account of its own (the Actual import's, or one a disconnect left) attaches as tracked, keeping its role. Undoing the import that made such an account puts its feed account back in the prompt.
- **Stopped arriving** means absent from the newest response (`connection_accounts.last_seen_at` older than the newest).

**Balances-only accounts** (investments, loans): only their balance is synced; any transactions the feed sends for them are ignored.

## The connection

One connection per household: the SimpleFIN access URL. **Settings › Bank connection & sync** claims it:

1. "Get a setup token from SimpleFIN Bridge and paste it here." The token is base64 of a claim URL; the server decodes it (or accepts the claim URL itself if pasted), checks it starts with `https://`, and `POST`s to it once with an empty body. A token that doesn't decode is refused without any request.
2. The response is the access URL (`https://user:pass@host/path`). It is encrypted with `BUDGET_SECRET_KEY` (XChaCha20-Poly1305 *(agent)*) into `connections.secret_encrypted`, never displayed again, never logged, never in an API response.
3. The first sync runs at once as a back-fill (below), and the new accounts prompt fills.

**Replace setup token…** claims a new token onto the same connection (after a revoke, a lapse or a leak). Accounts re-match by provider account id; any that don't come in through the prompt with **This is the same as…**. Disconnecting deletes the secret and stops syncing; accounts and rows stay, and become manual accounts *(agent)*.

Details of the claim *(agent)*:

- **Setup token forms:** base64 (standard or URL-safe, padded or not) of the claim URL, or the claim URL itself. A claim URL with a login in it is refused, since the real ones never carry one.
- **Claim request:** one `POST` with an empty body, a 30 s timeout, redirects not followed, `User-Agent: saltcellar/<version>`. The answer must be an access URL (`user:pass@host`) on a host the claim policy allows, else it is refused.
- **Errors:** a token that isn't one is a 422 `validation_failed` on `setup_token`; a 403 from the Bridge (a token is good once) is 422 `token_refused`; no answer, another status or an answer that isn't an access URL is 502 `bridge_unavailable`; no `BUDGET_SECRET_KEY` is 409 `secret_key_missing`. None of the messages carry the token, the access URL or the HTTP library's own text.
- **Sealing:** XChaCha20-Poly1305 with a fresh random 24-byte nonce per value, stored as one version byte, the nonce, then the ciphertext and tag, with the purpose bound in as associated data (so a value sealed for another purpose, the kept raw responses later, can't be opened as an access URL).
- **The key check** runs at startup: a connection whose secret `BUDGET_SECRET_KEY` can't open (missing or different key) is marked `key_mismatch` and the app starts; start again with the right key and it goes back to `ok`. Pasting a new token under the current key also clears it. An unreadable `BUDGET_SECRET_KEY` (not 32 bytes of base64) stops startup, naming the variable without echoing it.
- **Replace** claims onto the same `connections` row: the secret is swapped, the status and pause cleared, and the accounts stay linked. **Disconnect** deletes the row and the secret and sets `connection_id` to NULL on its accounts (they keep their provider account ids, so connecting again re-attaches them); it asks first and does nothing when there is no connection.
- **The accounts table** was rebuilt by migration `0011` to give `connection_id` its foreign key, outside sqlx's transaction with foreign keys off for the rebuild, as SQLite's documentation prescribes.

`BUDGET_SECRET_KEY` is required to claim a token; without it the claim screen says so and does nothing. If a connection exists and the key is missing or wrong, the app still starts, sync shows **Sync paused** with "The secret key changed. Paste a new setup token." and nothing else is affected *(agent)*. Losing the key costs a new setup token, never data.

### The request

`GET <access URL base>/accounts?version=2&pending=1&start-date=<epoch seconds>`, with HTTP Basic auth from the URL's user and password and a `User-Agent: saltcellar/<version>` header (the Bridge's CDN blocks default client agents). All accounts in one request; never `?account=` (it has a separate, smaller quota) *(agent)*. No `end-date`. Timeout 60 s *(agent)*.

- **Daily sync, Sync now, retries, catch-up:** `start-date` = now − 30 days.
- **First connection (back-fill):** 90 days as two requests of 45 days each; older history comes from the Actual import or files. No request ever asks for more than 45 days.
- **Back-fill details** *(agent)*: the newer half has no `end-date`, like every other request; the older half ends where the newer starts (`end-date`, exclusive), so it alone carries one. An account is back-filled when it is tracked (the Track request answers once its rows are in), and after a claim for accounts attached at once; `connection_accounts.backfilled_at` records it done, so one moved onto a new id is back-filled again. While any tracked account still needs it, a run makes the back-fill's two requests: the newer half serves every tracked account, the older only those. A back-fill that fails is retried by the next sync, and tracking stands.
- Around 2–4 requests a day in normal use.

### Reading the feed

What the protocol and the live Bridge actually do, so code doesn't rediscover it:

- **Shape (v2):** top level `errlist`, `accounts`, `connections`. Each account carries `conn_id` (its bank link), `id`, `name`, `currency`, `balance`, `available-balance`, `balance-date`, `transactions`, `holdings`. Each transaction carries `id`, `posted`, `amount`, `description`, `payee`, `memo`, `transacted_at`, and `pending: true` only when pending (posted rows omit the key). `mcc` exists but has always been null.
- **Amounts** are decimal strings; convert to minor units with the household currency's decimal places. A value with more decimals than the currency allows fails that account for the run *(agent)*. An account whose `currency` differs from the household's is refused with a clear error and never tracked ([ADR-0006](../adr/0006-one-currency-one-timezone.md)) *(agent)*.
- **Pending:** a row is pending when `pending` is `true`. Never infer it from `posted == 0`: some banks send a future `posted` date on pending rows.
- **Dates:** both `posted` and `transacted_at` are epoch seconds whose encoding varies by bank (a date at noon UTC, a date at local midnight Eastern, real clock times). Take the **UTC calendar date**; converting to the household timezone shifts some banks' dates back a day. A row's `date` is the UTC date of `posted`, or of `transacted_at` while pending *(agent: the rule mature clients use; it keeps dates equal to the Actual import's for the same rows)*.
- **Window edge:** the Bridge cuts the window at the exact second on `transacted_at`, not on `posted` and not at day precision. A row transacted just before `start-date` is missing even if it posted inside the window. Hence the overlapping 30-day window.
- **Order** of rows in the response is not reliable.
- **Ids:** account ids (`ACT-…`) and transaction ids (`TRN-…`) are opaque. The protocol forbids reusing a transaction id within an account but doesn't promise one transaction keeps one id: ids have been seen re-issued over days at some banks, and re-linking a bank can change them. Hence the content fallbacks below.
- **Text:** `description` (raw bank text) is stored as `raw_description`; `payee` (the provider's cleaned name) feeds the payee. Text can change on posting or a day later under the same id (a truncated name completed, a date token dropped); sync owns it, so it updates.
- **Balances:** `balance` is what the bank reports, and never includes pending rows at the banks observed. Liabilities are negative. `available-balance` is only meaningful on depository accounts (balance plus pending there; 0 on cards, loans and investments) and is never stored *(agent)*. `balance-date` is when the provider last refreshed that bank link, so it is the staleness signal.
- **An account can be missing** from a successful response, with an empty `errlist`, and come back the next day unchanged. Missing never means closed (below).

### Errors

`errlist` entries are `{code, msg, conn_id?, account_id?}`; an unknown subcode falls back to its prefix. Every message is shown to the household, sanitized (no access URL, no amounts) *(agent: control characters stripped, 300 characters max)*.

| Signal | Meaning | What the app does |
|---|---|---|
| network error, HTTP 5xx, `act.failed`, `act.missingdata` | transient | Retry (below). Those accounts keep their last data. |
| `con.auth` (with `conn_id`) | one bank link needs re-authorizing at the Bridge | That bank's accounts show **needs reconnecting** with **Fix at SimpleFIN ↗** (the Bridge's connections page), then **Done, sync now**. Not retried. |
| other `con.*` | bank link error | Shown on its accounts; retried like a transient error *(agent)*. |
| HTTP 403 or `gen.auth` | access URL revoked or disabled | Connection `token_revoked`: all syncing pauses, banner on every screen. Not retried. |
| HTTP 402 | Bridge subscription unpaid | Connection `unpaid`: paused as above. Not retried. |
| `gen.api`, or anything else without a `conn_id` or `account_id` | developer-facing notice (e.g. a long-window warning) | Logged on the run; nothing shown *(agent)*. |

## Sync runs

- **Schedule:** once a day at `sync.time` in the household timezone, default **06:13** (after the provider's overnight refresh; off the hour, as the Bridge asks). Changeable in Settings › Bank connection & sync; the UI says it loosely ("syncs daily around 6 am"). Retries and catch-ups also use off-the-hour minutes.
- **Sync now:** a 30-minute cooldown after any sync ("Synced 12 min ago"). **Done, sync now** after a re-authorization skips the cooldown.
- **Request budget:** the app makes at most **12** Bridge requests in any rolling 24 hours, across every trigger (scheduled, manual, retry, catch-up, back-fill), half the Bridge's limit of 24. One is always held back for the scheduled sync. At the cap, Sync now reads "Sync limit reached, try again after 3:40 pm". Counted from `sync_runs.requests`.
- **Retries:** only transient failures, at about +20 minutes and +2 hours, all accounts in one request, counted against the cap; then wait for the next day.
- **Catch-up:** on startup, if the last successful scheduled sync is over 20 hours old, a catch-up sync runs a few minutes after boot; the next scheduled run still happens.
- **One at a time:** a second trigger (another login, a retry, a catch-up) joins the running sync; never a second Bridge call. A back-fill for a newly tracked account waits for the running sync and then runs, since that sync didn't include it *(agent)*.
- **Partial failure:** accounts that succeeded commit; each failed account keeps its last data and shows "Last updated Oct 21 · needs attention". A failed account, or one missing from the response, doesn't count toward pending retirement, `not_seen_at_bank` or drift for that run.
- **Stale and needs attention:** after a successful sync, an account whose `balance-date` is more than **2 days** old is **stale** ("data is 3 days old"; quiet, no retry); more than **5 days**, it **needs attention** with the reconnect button. A tracked account missing from responses for more than 5 days also needs attention *(agent)*.
- **Paused:** while the connection is `token_revoked` or `unpaid`, no scheduled calls; a banner on every screen: "Bank sync stopped. Get a new setup token from SimpleFIN and paste it in Settings" (or "SimpleFIN subscription unpaid"). Replacing the token clears it.
- **Run history:** every run, with per-account results (ok, error code, sanitized message, `balance-date`), counts added and updated, and requests made, kept 90 days, listed in Settings › Bank connection & sync. One container log line per run (trigger, counts, requests, error codes), never amounts, merchants or the access URL.
- **Raw responses:** only behind **Keep raw bank responses for troubleshooting** (off by default), kept 7 days, encrypted with `BUDGET_SECRET_KEY`, never logged.
- **Details of the schedule** *(agent)*:
  - The rules are pure functions in `domain::sync_schedule`, tested on a fake clock; the API's `schedule` module runs them, looking for due work every 30 s. What waits (a retry, this start's catch-up) is kept in memory: a restart forgets retries, and the catch-up stands in for them.
  - The scheduled run starts within 1 hour of its slot, once; later than that, only a restart's catch-up makes up for it. A time a clock change skips runs just after it. Changing the time to one already past today runs it tomorrow.
  - The cooldown counts from the newest finished run that isn't a back-fill, so tracking an account doesn't lock Sync now. Sync now answers 409 `sync_cooling_down` ("Synced 12 min ago. Sync now works again in 18 min.") or 409 `sync_limit_reached` ("Sync limit reached, try again after 3:40 pm.", in the household timezone). A press while a sync runs still joins it.
  - Every run asks the budget for the requests it will make (a back-fill's two included); only the scheduled run may take the held-back twelfth. Claiming a setup token isn't a sync run and isn't counted. The cap is `BankConfig::sync_request_cap`, raised only by the development setting below.
  - Transient: no answer, a 5xx, `act.failed`, `act.missingdata`, and `con.*` but `con.auth`, on the run or on any account. Retries follow scheduled, catch-up and retry runs only; Sync now and back-fills show their result instead. The second retry is 2 hours after the first failure, not after the first retry. Any successful run cancels a waiting retry; a non-transient failure ends them.
  - The catch-up runs 3 minutes after start when the last successful run of any trigger (a run without a run-level error) is over 20 hours old: a fresh Sync now makes it pointless. It checks again when due.
  - Retries and catch-ups that would land in the first 3 minutes of an hour move to minute 7.
  - Run history and raw responses are pruned after every run. Turning raw responses off deletes the kept ones. They are sealed like the access URL but bound to their own purpose (`simplefin::seal_raw_response`), in `sync_raw_responses` (migration `0016`).
  - The API: `GET /api/v1/bank-connection` carries `sync` (time, keep raw, cap, requests left, last synced, cooldown and limit ends); `PUT /api/v1/bank-connection/sync-settings` (`sync_time` `HH:MM`, `keep_raw_responses`; 422 on `sync_time`); `GET /api/v1/bank-connection/sync-runs` (newest first, each run with its accounts' results).
  - The screen says the time rounded to the nearest hour, in the login's locale ("around 6 AM"). Recent syncs lists each run's time, trigger, summary and requests, and each failed account's message.

## Matching a synced row

Each row in the response is matched to a stored row in this order; the first step that matches wins.

0. **Deleted by the household:** an id in `deleted_bank_rows` for that account is skipped.
1. **Same id:** `(account, external_id)`. Update the sync-owned fields.
2. **Pending → posted:** a posted row with no id match takes over a stored pending row on the same account when the pending row isn't in this response, the posted date is within 7 days after the pending date, the description is similar, and the amount is equal or up to 25% larger in size (tips). One candidate: the posted row replaces the pending one and inherits everything the household set (category, memo, tags, splits, payee). Several: pair oldest first on exact amount only; otherwise insert new and let the pending rows retire normally.
3. **Re-issued id:** exactly one stored row on the account with the same amount, a similar description and a date within ±1 day, whose own id wasn't in this response → the same transaction; record the new id. Two or more → insert as new with `sync_flag = 'possible_duplicate'` and `possible_duplicate_of`, for the household to merge or keep.
4. **Adopt an imported row:** an imported row with no `external_id` on the same account, exact amount, date within ±3 days (identical rows counted, not collapsed; a tie → `possible_duplicate`) is taken over.
5. **Adopt a manual row:** exactly one manual row with no `external_id` on the same account, exact amount, bank date from 3 days before to 14 days after the manual date, is taken over. Two or more → insert as new, `possible_duplicate`. No exact match, but exactly one manual candidate in the window and the bank amount is up to 25% larger in size → insert, and flag the bank row `possible_manual_match` ("Is this your $X manual entry?"); accepting merges them, keeping the household's fields and the bank's amount. Never automatic. "One manual candidate" counts only the manual rows in the window the bank amount exceeds by up to 25%; a manual row of another amount in the window doesn't block the flag *(agent)*. A flagged manual row stays free: a later bank row of its exact amount can still adopt it *(agent)*.
6. **Insert** as a new row, `source = 'provider'`.

"Similar description" *(agent)*: after the shared normalization (below), one text starts with the other, or they share their first word and their first 8 characters *(agent)*. Pending text is often truncated.

Details of matching *(agent)*:

- **What a run matches against:** every row the run's response(s) held for the account (both back-fill halves together), applied once per run and only for an account that succeeded. Stored candidates are the account's rows dated from a week before the earliest incoming row, plus any carrying an incoming id.
- **Order:** step 1 for every row first; then the rest oldest first (by date, then id), each stored row taken at most once, so several pending rows posting at once pair oldest with oldest. "Several, none exact" in step 2 inserts without trying steps 3 and 4.
- **Step 3** compares a pending row only with pending rows and a posted one only with posted, and needs the stored row to have a bank id; the flagged row points at the oldest candidate.
- **Step 4 ties:** the nearest date wins, then a similar description; candidates equally good that are copies of each other (same date and normalized text) are taken one per bank row; equally good ones that differ are a tie, flagged against the first.
- Matching code is `domain::matching`; the writes are `db::apply_synced_rows`.

Live behaviour behind these rules: at the banks observed, a pending row normally **keeps its id** when it posts (step 1), usually within a day, at most three; a new id appeared only when the amount changed (a restaurant tip), with the same `transacted_at`, which step 2 catches. No posted id was re-issued in the observation period, but other banks do it, so step 3 stays.

**Adopting** a row (steps 4 and 5) gives it the provider id and `source = 'provider'`; from then on sync owns its amount, date, status and text. The household's category, memo, payee, splits and tags stay.

### Who owns which field

- **Sync owns** amount, date, pending/posted status and raw description. It updates them on every run.
- **The household owns** payee, category, splits, memo, tags and exclude-from-budget. Sync never changes them once set.
- An **unsplit** row whose amount changes keeps its category. A **split** row whose amount changes is never adjusted silently: it gets `sync_flag = 'amount_changed_split'`, "Amount changed from $X to $Y, fix the split", and in milestone 3 goes back to Review.

### Rows the bank stops sending

- **Vanished pending:** a pending row is removed after **two consecutive successful syncs** that returned its account but not the row (`pending_miss_count`). Runs where its account failed, had `act.missingdata` or was missing don't count. A removed row the household had edited goes to **Recently dropped by bank** (under Transactions' filters *(agent)*), undoable for 30 days (`dropped_transactions`). Holds that never post (a hotel deposit) end this way.
- **Posted rows missing:** never deleted automatically. A posted provider row whose date lies inside the fetch window, at least 5 days after its start *(agent: the margin covers rows transacted before the window that posted inside it)*, and that hasn't been returned for 7 days while its account kept succeeding, gets the quiet `not_seen_at_bank` flag; the household deletes or keeps it.
- **Missing accounts:** an account missing from a successful response counts as failed for that run (error code `absent` *(agent)*): its data stays, its rows' miss counts don't move.

Details *(agent)*:

- **Edited**, for a removed pending row: categorized by hand, changed by a login (`updated_by`), or given a memo, a split, a tag or exclude-from-budget. Unedited removed rows are kept in `dropped_transactions` too, with `user_edited = 0`, so they aren't listed. Every dropped row is pruned 30 days after it went, at the next sync.
- **Recently dropped by bank** *(agent)*: a chip beside **Needs a look** that shows the dropped rows in place of the list (turning it on clears the other filters), the latest first, each with **Restore** (`GET /api/v1/dropped`, `POST /api/v1/dropped/{id}/restore`). A restored row comes back from its snapshot with the household's edits, under its old id when that's free, and opens in its detail; its bank id stays, so the bank sending or posting it still matches it. It is **kept by the household** (`transactions.kept_by_household`): sync no longer counts its misses, so it isn't dropped again. A payee, category or tag deleted since is left off; the transfer it was a leg of doesn't come back. If the bank has sent its id again since, Restore refuses (`409 back_from_bank`): it's in the list already.
- **The fetch window** for `not_seen_at_bank` is the earliest window that brought the account's rows that run (the back-fill's older half when it ran). The flag is only set on a row with no other flag, and goes when the bank sends the row again.
- A removed or deleted row that another row is flagged a possible duplicate of leaves that row unflagged.

### Deleting a synced row

Allowed, with a warning that the bank still has it and a suggestion to exclude it from budgets instead. Its id goes into `deleted_bank_rows`, so the daily window never brings it back.

Details *(agent)*: **Delete** is in the transaction's detail panel for rows from the bank; it opens the warning with **Exclude from budgets instead** (hidden once excluded), **Delete** and **Cancel**. There is no Undo, since the row can't be entered again. The API deletes any row with a bank id this way (an Actual row with one included), and refuses a file row without one (`not_manual`: undo its import instead). Excluding is `PUT /api/v1/transactions/{id}/excluded`; the detail shows **In budgets: Counted / Excluded** with **Count it again**.

### Payees for synced rows *(agent)*

Until milestone 3's normalizer, a synced row's payee comes from the feed's `payee` field (else `description`) the same way the Actual import does it: an existing payee alias, else a payee by exact name, created if new, with the minimal normalization as its alias. Milestone 3 re-runs the normalizer over rows whose payee the household didn't set.

## The dedupe key

`transactions.dedupe_key` is a SHA-256 (lowercase hex) of account id, date, amount in minor units and the description after the **shared normalization**: lowercase, whitespace collapsed, punctuation stripped. It is the same function milestone 1 introduced, now final for every source (sync, files, manual, Actual) *(agent)*. The normalization is deliberately frozen: milestone 3's payee normalizer can improve without changing stored keys.

It answers one question cheaply: "is this exactly the same row?" Identical rows (same key) are counted, not collapsed, when a file or the Actual import matches against stored rows. The windowed matches above (±1, ±3, −3/+14 days) query by account, amount and date range instead.

## Balances

The bank's balance, stored as a **balance snapshot**, is the balance shown for every account. The app's own balance (opening balance + transactions + balance adjustments) only detects **drift** (bank − app). The goal is that the two match, and that every gap can be traced to a sync.

- **Snapshots:** from `balance` only. Written only when `balance-date` moves forward, at most one per account per UTC day (a later refresh that day replaces it). Each stores the bank balance, the app's posted-only and with-pending balances, the run that wrote it, and `sign_corrected`. Kept forever.
- **Which app balance:** `balance` excluded pending rows at every bank observed, so drift compares with the **posted-only** app balance. `accounts.balance_includes_pending` is learned per account: NULL until a snapshot matches exactly one of the two app balances, then 0 or 1 *(agent)*. While NULL, an account is flagged only when neither matches.
- **Drift flag:** non-zero drift that holds at the same value for **2 consecutive snapshots** shows "Balance off by $X since <date>" on the account. The date is when drift last stepped; the rows that run added, changed or dropped are listed as **suspect rows**. Failed runs, and runs where `balance-date` didn't move, write no snapshot and don't count. The flag clears itself when drift returns to zero. The app never adjusts on its own.
- **Balance adjustment:** fix rows where possible; otherwise **Accept $X as balance adjustment** records one (account, date, amount, note, who). It is not a transaction: never in spending, budgets, Review or transfer matching. Undo keeps the record, so a later drift report can count how often drift went unexplained.
- **Opening balance:** derived on an account's first sync when it has none: first bank balance − the back-filled rows, dated the day before the earliest one. Stored on the account, never as a transaction. When older history arrives (Actual, files), the date moves back and the amount is recomputed so drift doesn't change.
- **Sign bugs:** the provider sometimes reports a balance with the wrong sign while the rows are right. On an account with transactions, a bank balance whose sign differs from the app's posted-only balance, where the flipped value is closer to it, is flipped on save and marked `sign_corrected`, unless both are within one major unit of zero *(agent: the near-zero crossover)*. On a balances-only loan, a positive balance is flipped *(agent)*.
- **Balances-only accounts:** snapshots only. No opening balance, no drift, no flag; only stale and needs attention apply.
- **Fund-backing accounts:** a fund's real balance (milestone 5) is its backing account's latest snapshot. Drift is flagged on the account, never on the fund.

## Manual entry

Mainly for entering something on a synced account before the bank sends it (a check, a payment app transfer); also for manual accounts.

- **Entry form** (Transactions, + button): account, date, amount (money out by default *(agent)*), payee, **category (required)**, memo, tags, or **Transfer to…** another account. Category suggestions only, no pipeline, so a manual row never reaches Review. Pending-like: it counts in budgets and reports at once, with a **manual** badge.
- **Before adoption:** every field is editable and matching uses the edited values; delete is immediate with an undo toast.
- **Adoption:** step 5 above. After it, the row behaves like any synced row and the badge goes.
- **Never matched:** a manual row on a synced account still unmatched 14 days after its date gets `not_seen_at_bank`; delete or keep. Never deleted automatically. Drift points at it too. "14 days after" counts from the sync's UTC date: flagged once its date is more than 14 days before it, when no bank row can adopt it any more; an edit that dates it inside the 14 days again clears the flag at the next sync *(agent)*.
- **Transfers:** **Transfer to…** writes a one-leg transfer (`matched_by = 'manual'`) whose other leg fills in when that account syncs. Otherwise a manual row pairs with a synced leg only as a **Transfer candidate** (milestone 3), never automatically.
- **Entry details** *(agent)*: a manual row is `status = 'posted'`, `categorized_by = 'user'` with the reason "Set by <display name>"; its `raw_description` is the payee's name (empty without one), and its `dedupe_key` comes from that. The + opens the form where a transaction's detail opens (a sheet on phones), with the account filter's account chosen if one is set, dated today in the household's timezone; an entered row opens in its detail. Payee is free text offering the existing payees; a new name adds a payee. Tags are typed separated by spaces or commas, `#` optional, letters, digits, `-` and `_`. Amounts are typed without a sign, with **Money out** / **Money in** beside them. Balances-only and closed accounts take no manual row; **Transfer to…** offers every other open account, balances-only ones included. Money out goes from the row's account to the other one; money in comes from it ("Transfer from <account>"). The transfer is `matched_by = 'manual'`, `state = 'confirmed'`, `confirmed_by` the login. A row stays editable while `source = 'manual'` and it has no `external_id`; the API's `manual` flag says so and drives the badge. Editing rewrites its tags and transfer. Delete removes it with its tags and transfer; the **Undo** toast (8 seconds) enters the same fields again as a new row, so it gets a new id. Category suggestions wait for milestone 3's pipeline. The API: `POST /api/v1/transactions` (`201`), `PUT` and `DELETE /api/v1/transactions/{id}` (`409 not_manual` for any other row, `404 not_found`), every field error as `422 validation_failed` by field, and `GET /api/v1/payees`.
- **Manual balance snapshots:** only on accounts with no working sync: manual accounts, and balances-only accounts whose bank link is broken or gone. On a manual account with transactions they behave like bank snapshots (drift, accept as balance adjustment). Never on a syncing account. **Update balance** on the account's detail enters one, dated today; a second the same day replaces the first, as a bank's later refresh does *(agent)*. Until bank links exist, the rule is "no connection": the API refuses one on any account with `connection_id` set (`account_connected`), and the broken-link case arrives with balances-only sync health *(agent)*. An amount is typed the locale's way, its group marks only between groups of three, so "12.50" isn't twelve hundred in German *(agent)*.

## CSV and OFX/QFX import

**Import a file** (an account's detail, and More *(agent)*). Any account can take one, including manual accounts. Each commit is an **import batch**, undoable from Settings › Imports.

- **Formats:** CSV with a column mapping, and OFX/QFX/QBO through one OFX parser. No PDF. No built-in bank presets.
- **Account:** chosen at import; pre-selected by an OFX account's last 4 digits, or by a CSV header already mapped on an account.
- **CSV mapping**, saved per account on first import and reused when a file with the same header arrives: date column (+ optional post date), description, amount as one signed column or debit/credit columns, flip sign, date format (guessed from the rows, confirmable), optional category and memo columns, header rows to skip.
- **Matching** against stored rows (synced, imported or manual): same account, exact amount, date within ±3 days of the file's transaction or post date; description only breaks ties. Identical rows are counted, not collapsed. A tie with no tiebreaker shows as **possible duplicate** for the household to decide. A FITID or CSV row hash (`import_ref`) from an earlier batch short-circuits matching on re-import. A posted file row matching a pending synced row counts as matched; sync posts it as usual.
- **Matched rows** keep their amount, date, status and text: a file never overwrites sync-owned fields. It can only add a **bank category**.
- **Preview before commit:** "N new, M already there, K to check", with the K rows to decide. Nothing is written until **Import**.
- **Bank category:** a file's category column is stored as `bank_category` on new and matched rows. Rules can match it (milestone 3), e.g. an always-review warehouse club + bank category "Gas" → Gas. A row gaining a bank category re-runs the pipeline unless the household set its category.
- **New rows** run the full pipeline from milestone 3 on, like synced ones, transfer detection included (pairing with synced or imported legs). Rows dated more than 90 days before the import that end up uncategorized are quiet (`review_quiet`): out of Review, under the **Imported, uncategorized** filter.
- **Balances:** a file never writes a balance. Rows older than the opening date move the opening balance back (drift unchanged).
- **Undo** deletes the rows the batch inserted (asking first if any were edited) and clears bank categories it set. It never deletes rows sync has since adopted.
- **Sync adopts imported rows** (step 4 above).

## Import from Actual becomes overlap-aware

Actual may itself have been syncing from SimpleFIN, so its accounts and rows can carry the same provider ids the connection sends. Milestone 1's wizard gains overlap handling:

- **Accounts step:** each Actual account is pre-matched to the app account, or the connection account waiting in the new accounts prompt, with the same provider account id; the household confirms, overrides, creates new or skips. Name matching or a manual pick covers accounts without an id. An Actual account with no feed counterpart can become a manual account. Order doesn't matter: imported first, the accounts carry their provider ids and the first sync attaches them as tracked.
- **Same id:** a shared (account, transaction id) is the same row. The synced row keeps its amount, date, status and text; Actual adds its mapped category, notes and payee, unless the household already set the category (`categorized_by = 'user'`), in which case Actual's category is dropped. Fields filled this way are recorded in `prior_fields` and restored on undo.
- **No id match:** inside the period the connection has synced, the file rule applies (same account, exact amount, ±3 days, repeats counted, ties shown as possible duplicates in a preview step before Done *(agent)*). Rows older than the oldest synced row are inserted.
- **Ids kept:** imported rows carry Actual's provider transaction id as `external_id` (`source = 'import'`), so a later sync or back-fill matches them by id.
- **Re-import:** the milestone 1 refusal ("Undo the earlier import first") is lifted: a second Actual import matches its rows against the first by id like any other overlap *(agent)*.
- **Transfer-mapped and uncategorized rows** are handled by milestone 3's pipeline, as milestone 1 already describes.

- **Details of errors and health** *(agent)*:
  - The rules are pure functions in `domain::sync_health`, tested at the 2- and 5-day edges (strictly more than 48 and 120 hours). `con.auth` is read by prefix and subcode (`con.auth.*` too); `gen.auth` in a 200 answer's `errlist` pauses the connection like a 403, with the run's error code `gen.auth`.
  - An account's health comes from its newest result and the newest `balance-date` any result had, read when asked, not stored. Order: reconnecting (`con.auth`), failed read (any other error on the account or its bank link), missing over 5 days, data over 5 days old, stale. A failed read needs attention at once ("Last updated … · needs attention"), but offers no **Fix at SimpleFIN**: only reconnecting, missing and old data do, since the household can act on those at the Bridge. An account absent for under 5 days stays quiet.
  - A bank link is the feed's `conn_id` (`connection_accounts.bank_link_id`, migration `0017`); its state is its worst account's (reconnecting, needs attention, stale, balances only when every tracked account is, else ok). Its row shows the bank's name, tracked accounts, a line in words and the newest refresh.
  - **Done, sync now** is `POST /api/v1/bank-connection/sync-after-reconnect`: a Sync now without the cooldown; the cap and joining a running sync stand, and a paused connection answers 409 `sync_paused`.
  - Accounts carry `sync` (state, message, days, last refresh, fix) and `balance_entry`. A balances-only account takes **Update balance** while its bank link is broken (reconnecting, missing, old data) or the connection is paused; `balance_entry` says `manual` then.
  - **Fix at SimpleFIN ↗** opens the Bridge's sign-in page, `https://bridge.simplefin.org/auth/login`, in a new tab: the Bridge has no stable link to its connections page.
  - Unscoped notices (`gen.api` and the like) put their codes in the run's log line and nowhere else. The paused banner reads "SimpleFIN subscription unpaid. Bank sync stopped. Pay it, then paste a new setup token in Settings." for `unpaid` and names the secret key for `key_mismatch`. The connection screen refetches every minute, so the banner shows when a sync pauses it. Accounts list: a ⚠ on accounts that need attention; the account screen shows the explanation and the fix. The status spot and sheet stay with the sync health issue.

## Sync health in the UI

**One status spot**, never warnings scattered over every screen:

- **Where:** a pill in the phone header, and a button at the foot of the desktop sidebar. All good: "Synced 6:13 am" on desktop, "6:13 am" on phone, with a hollow dot. Problems: a filled dot and "1 sync problem" / "2 sync problems" (only needs-attention problems count), or "Sync paused".
- **Sync sheet**, opened from the spot: the last-sync line ("Synced 6:13 am · syncs daily around 6 am"); every problem with its account, a short state, one sentence of explanation and its fix (**Fix at SimpleFIN ↗**, **See suspect rows**, **Paste new token**); quiet states (stale data) listed without counting toward the pill; **Sync now** with its cooldown and "N of 12 requests left today"; a link to the run history.
- **Overview, Review, Transactions:** nothing extra.
- **Accounts list:** a problem account shows only a ⚠ marker.
- **Account detail:** the full explanation and fix. Drift shows the bank's balance against the app's, **See suspect rows** and **Accept $X as balance adjustment**. The footer gives the last sync and "bank data as of <date>".
- **Paused banner** on every screen while the connection is paused.
- **Sync flags on rows** (`possible_duplicate`, `possible_manual_match`, `not_seen_at_bank`, `amount_changed_split`) show as a badge and an action in the transaction detail, and under a **Needs a look** filter in Transactions *(agent)*.

Details of the flag actions *(agent)*: every action clears its flag (`POST /api/v1/transactions/{id}/resolve` with `merge`, `keep_both`, `accept_match`, `reject_match` or `keep`; `409 not_flagged` when the row doesn't carry the flag the action answers). The badges read **Duplicate?**, **Manual match?**, **Not at bank** and **Fix split**.

- **Merge** (`possible_duplicate`) and **Yes** (`possible_manual_match`, "Is this your $X manual entry?") merge the two rows: the flagged row, the newer one with the bank's id, amount and date, survives and takes the household's fields from the row it points at (its category, split lines or transfer, payee, memo and tags, exclude-from-budget), which is deleted; a bank id it had goes into `deleted_bank_rows`. Split lines that no longer add up flag the survivor `amount_changed_split`. Another row flagged a possible duplicate of the deleted one points at the oldest other row it could be (same account, amount and status, within 3 days, a similar description, not itself a new possible duplicate), so rows re-issued together are resolved one by one; with none, it is unflagged.
- **Keep both** and **No** clear the flag and the pointer.
- **Keep** (`not_seen_at_bank`) clears the flag and marks the row kept by the household, so sync doesn't flag it again. **Delete** is the row's ordinary delete: a manual row goes as manual rows do, a bank row as in [Deleting a synced row](#deleting-a-synced-row), without the warning, since the bank no longer sends it. Deleting a manual row a bank row is flagged against leaves that row unflagged.
- **Fix the split** (`amount_changed_split`, "Amount changed from $X to $Y, fix the split", $X being the lines' sum) opens the lines in place: category, amount (typed in the row's direction) and memo per line, lines added or removed, what's left to split shown; saving needs them to add up (`PUT /api/v1/transactions/{id}/splits`, `422` otherwise) and clears the flag. One line leaves the row unsplit in that category. Review takes this over in milestone 3.

## Settings › Bank connection & sync

- The connection: "SimpleFIN Bridge · connected", "Syncs daily around 6 am · 9 of 12 requests left today", **Sync now**, **Replace setup token…**, **Disconnect** *(agent)*, and the sync time.
- **Banks:** one row per bank link with its accounts count, state (ok, balances only, stale with "bank hasn't updated in 4 days", needs reconnecting with **Fix at SimpleFIN ↗**) and last refresh.
- **Recent syncs:** the run history, newest first.
- **Keep raw bank responses for troubleshooting** checkbox.
- The screen also shows the last run in a line ("Last sync: 3 rows added, 1 row updated", "No changes", or the run's error) under the connection *(agent)*.

## Configuration added

| Variable | Default | Meaning |
|---|---|---|
| `BUDGET_SECRET_KEY` | none | 32 random bytes, base64 (`openssl rand -base64 32`). Encrypts the connection's access URL and kept raw responses. Required to claim a setup token. Keep a copy outside the data dir; backups don't include it. |

| `BUDGET_SIMPLEFIN_CLAIM_HOSTS` | none | Development only *(agent)*. A comma-separated list of `host[:port]`. When set, a setup token may point only at these hosts, over `http://` as well as `https://`, so the [fake SimpleFIN server](../../fixtures/simplefin-fake/README.md) works. Unset (production), any `https://` claim URL is accepted. |
| `BUDGET_SYNC_COOLDOWN_MINUTES` | `30` | Development only *(agent)*. Sync now's cooldown in minutes; the e2e tests set `0`, since their specs press Sync now minutes apart. |
| `BUDGET_SYNC_REQUEST_CAP` | `12` | Development only *(agent)*. The Bridge requests allowed in any rolling 24 hours, 2 or more. The e2e tests set `100`, since their specs sync more often than a real install may. Leave unset in production: the real Bridge allows 24. |

Settings keys: `sync.time` (`"06:13"`), `sync.keep_raw_responses` (`false`).
