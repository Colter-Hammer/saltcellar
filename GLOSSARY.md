# Saltcellar

A self-hosted, Mint-style personal finance tracker: money is tracked against targets, not assigned to envelopes.

## Language

**Household**:
The one group of people sharing a budget in an install. It has one currency and one timezone; "today" and budget windows follow that timezone.
_Avoid_: Tenant, organization

**Login**:
One person's way into the household's budget. All logins are equal and see everything; there is no admin and no privacy split.
_Avoid_: Admin, member, account (that's a bank account)

**Category group**:
A named container of categories (e.g. Food) used for roll-up reporting and optionally budgeting. Never holds transactions directly.
_Avoid_: Parent category, master category

**Category**:
The single classification a transaction (or split line) receives; always belongs to exactly one category group.
_Avoid_: Subcategory, tag

**Budget**:
A spending target for a category or category group over a chosen period, compared against actual spending. Periods are monthly by default, or fixed windows of N months anchored to a chosen start month (e.g. a quarterly water bill: Feb–Apr, May–Jul…). A group budget is an overall cap; category budgets inside it are limits within that cap. A category budget on a multi-month window is tracked against its own window only and doesn't count toward a monthly group cap. Nothing carries over.
_Avoid_: Envelope, allocation

**Budget window**:
One period a budget is judged over: a month, or one of its fixed N-month windows. Target changes apply from a window forward; an override changes a single window.
_Avoid_: Budget period (that's the length, not one occurrence)

**Sinking fund**:
A fund attached to a budget: its monthly contribution is the budget's target spread across the window, and leftover money carries forward instead of resetting.
_Avoid_: Envelope, savings goal (that's a different thing)

**Fund**:
A running balance that builds up from contributions and is spent down by spending in its category (on any account), carrying forward across periods. The deliberate exception to "nothing carries over". Always computed, never read from an account: on paper, contributions follow a schedule; when backed by an account (which must be a budgeted account), contributions are the actual transfers into it, and the account's real balance is shown alongside.
_Avoid_: Envelope, pot. Note: an "emergency fund" is not a Fund; it is a tracking account with an ongoing balance goal.

**Play money**:
A fund belonging to one login: a fixed monthly contribution, spendable on anything, accumulating over time. Its spending counts as one line against that person's fund and never toward ordinary categories.
_Avoid_: Slush fund, allowance

**Tracking account**:
An account (savings, emergency fund, investments) whose balance is shown over time but whose activity never counts toward budgets. Money moved into one from a budgeted account counts as Saved, never as spending.
_Avoid_: Off-budget account

**Saved**:
The cash-flow bucket for net money moved into tracking accounts, alongside Income and Spending.

**Savings goal**:
A target for tracking accounts, of two kinds. A *contribution goal* is an amount to move in per period (under target is bad). A *balance goal* is an amount to hold, either by a date or ongoing; an ongoing balance goal is met while the balance is at or above it and becomes unmet again if money is withdrawn.
_Avoid_: Budget, sinking fund


**Payee**:
The merchant or counterparty as the household thinks of it ("Blue Bottle"), behind many raw bank spellings. Each payee is set to auto, always review, or never review.
_Avoid_: Merchant, description

**Auto-applied**:
A category the app set on its own, from a rule or a strong payee-history majority, without sending the transaction to Review. A person can still change it; a category a person set is never overwritten by the app.
_Avoid_: Auto-categorized by the classifier (the classifier only suggests)

**Uncategorized**:
A transaction with no category yet, usually waiting in the Review inbox. It still counts toward spending, as a temporary "Uncategorized" line, so totals never look better than reality.

**Undecided row**:
A row brought in by the Import from Actual wizard whose Actual category was mapped to "Decide later". It waits, grouped by its Actual category, in the "From Actual, undecided" list rather than in the Review inbox.

**Review inbox**:
The queue of transactions the app wasn't sure enough to categorize alone, worked through one at a time, oldest first.
_Avoid_: Inbox, queue

**Rule**:
A user-approved instruction: when a transaction matches all its conditions, take its actions (set category, rename payee, send to review, tag, mark as transfer). The app may suggest rules but never creates one by itself.
_Avoid_: Auto-rule

**Transfer**:
Money moved between two of the household's own accounts. Never spending or income; counts as Saved only when it enters a tracking account. A payment to a card the app doesn't track isn't a transfer: it is spending.
_Avoid_: Move, internal payment

**Transfer candidate**:
A possible transfer the app wasn't sure enough to match alone, waiting for the household to confirm or reject it. Until then neither side counts as spending or waits for review.
_Avoid_: Weak transfer, suggested transfer

**Connection**:
The household's single link to the bank-data provider, through which every synced account arrives. Distinct from a bank link: the provider's own login to one bank inside that connection. A bank link can need reconnecting while the rest keeps syncing; losing the connection stops all syncing until it is replaced.
_Avoid_: Integration, token (that's only its secret)

**Balance snapshot**:
The balance a bank reported for an account on a given day, or, for an account with no working sync, the balance the household entered. It is the balance the app shows for every account; for a balances-only account it is the whole record.
_Avoid_: Statement balance, available balance

**Drift**:
The gap between an account's balance snapshot and the balance the app works out from its own opening balance, transactions and balance adjustments. Non-zero drift means a row is missing, duplicated or wrong, or the bank changed the balance without sending a row.
_Avoid_: Discrepancy, reconciliation error

**Balance adjustment**:
An accepted amount of drift the household chose not to trace, recorded so the app's balance matches the bank's again. Never a transaction: it counts as neither spending nor income.
_Avoid_: Adjustment transaction, starting balance

**Opening balance**:
An account's balance just before its earliest transaction in the app, worked out from a balance snapshot rather than entered. Never a transaction or income.
_Avoid_: Starting balance (Actual's term, which was a transaction)

**History re-check**:
A pass over past transactions that proposes fixes for categories that disagree with a payee's strong majority. Changes apply only when approved.
_Avoid_: Re-categorization run

**Manual account**:
An account with no connection, whose transactions and balance snapshots are entered by hand (e.g. a wallet). If it later appears through the connection it becomes a synced account and keeps its history.
_Avoid_: Offline account, cash account (cash is only one kind)

**Import batch**:
One bank file (CSV or OFX/QFX) brought into one account in a single go, or the whole Actual export brought in by the Import from Actual wizard. It can be undone as a whole; undoing never removes rows the bank sync has since taken over.
_Avoid_: Upload, import job

**Bank category**:
The bank's own label for a transaction, carried in some export files (e.g. "Gas", "Shopping"). A hint that rules can match on; never one of the household's categories.
_Avoid_: Merchant category, MCC
