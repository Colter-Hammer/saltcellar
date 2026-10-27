# Milestone 3: Categorization + Review

**Goal:** categorizing is the biggest chore the app removes. Most transactions categorize themselves with very high precision; the rest wait in a Review inbox that is fast to work through, one transaction at a time; corrections teach the app.

**Done when:** new transactions from sync and file import go through the pipeline; the Review inbox, splits, the Transfers screen, Rules & payees, History re-check and the accuracy page work in the browser; the Playwright smoke test covers reviewing, splitting and confirming a transfer on fake data.

Measured on the owner's real history (a replay where each row was predicted only from earlier days), payee history alone auto-applied about 83% of rows at about 98% precision. That is the bar: auto-apply only what is that reliable; everything else goes to Review with good suggestions.

## The pipeline

Runs for each new transaction from bank sync and file import, and once over rows left uncategorized by the Actual import. Manual entry only gets suggestions (a manual row needs a category at entry). Pending rows are categorized right away; a pending row keeps its category when it posts.

1. **Normalize** the bank text to a payee.
2. **Transfer detection.** Matched legs skip every later step.
3. **Payee review setting.**
4. **Rules.**
5. **Payee history vote.**
6. **Classifier** (suggestions only).
7. **Decide:** a rule or history result auto-applies; anything else goes to Review with suggestions and a reason.

### 1. Normalize

Fold the bank's text into a payee spelling: strip processor prefixes (`SQ *`, `TST*`, `PAYPAL *` and similar), store numbers, city/state, long digit runs and ids; collapse case and punctuation; title-case. The bank feed often already supplies a cleaned payee name; when it does, normalize that and keep the raw description as a rule and classifier input.

- A known spelling (a `payee_aliases` hit) → its payee.
- An unknown spelling → a new payee is created automatically.
- A look-alike of an existing payee (same leading word(s) after normalization, or a small edit distance *(agent)*) → the review card shows "Same as <payee>? Merge". Never merged automatically. Merging repoints the aliases, so the spelling becomes an alias of the kept payee.
- Spellings that should land on the starter always-review payees (numbered checks → Check, ATM and teller withdrawals → Cash Withdrawal, and the listed merchants' common bank spellings) are built-in normalizer patterns *(agent)*.

### 2. Transfer detection

Equal and opposite amounts between two of the household's own accounts, close in time. **Strong** pairs (plus transfer-like text, such as the other account's last four digits) match automatically. **Weak** pairs become **Transfer candidates** on the Transfers screen, not in Review; until decided, neither leg counts as spending or waits for review. Matched legs carry no category.

Rows the Actual import marked as Transfer count as transfer-like text. A Transfer-marked row with no opposite leg becomes a Transfer candidate with two choices: **Pick a category** (a payment to a card the app doesn't track is spending) or **Transfer to <balances-only account>** (one-legged). Never automatic.

> **Placeholder:** the matching window, the exact strong/weak rules, lone legs waiting for a partner, one-legged transfers to balances-only accounts, and what happens when a leg changes are written with the bank sync sections.

### 3. Payee review setting

Every payee is set to one of:

- **Auto** (default): the normal cascade.
- **Always review**: never auto-categorized; always to Review, with suggestions and one-tap split. Seeded by the starter categories (see [milestone 1](m1-ledger-import.md#categories)).
- **Never review**: apply the top history pick even when history is mixed; only a first sighting with no history goes to Review.

### 4. Rules

A **rule** is user-approved: when a transaction matches **all** its conditions, take its actions.

- **Conditions:** payee is; raw description contains / matches (case-insensitive regex); amount is / between / in or out; account is; bank category is / contains.
- **Actions:** set category, rename payee, send to Review, add tag, mark as transfer to account X. No split actions in v1.
- **Order:** automatic, by specificity (more conditions and narrower ones first; e.g. payee + amount before payee alone) *(agent: specificity score = number of conditions, then exact before range before contains)*; `priority` breaks ties. No manual ordering.
- Rules are **never created by the app on its own**. It suggests them (below).
- A new or edited rule offers **Apply to N past transactions?**, touching only uncategorized and auto-applied rows.

### 5. Payee history vote

Over the payee's whole history (no window), among rows with a category:

- **Auto-apply** when the top category holds **≥ 90%** of the votes, from the payee's **2nd appearance** (at least one earlier categorized row).
- Below 90% → Review, with the history pick as the first suggestion and reason "mixed history".
- **Splits:** split lines don't vote for the payee; a split row counts as one "mixed" vote. A payee split about **1 in 4** times or more goes to Review automatically (from its 4th appearance *(agent)*).
- Corrections count immediately: history is computed from the current rows, so a fix in Review changes the next vote *(agent: computed on demand, no separate history table)*.

### 6. Classifier (suggestions only)

For payees with no usable history: a character n-gram TF-IDF + logistic regression model over the normalized payee, raw description and amount band, written in Rust. It **never auto-applies** in v1 (on the owner's history it was right only 42% of the time top-1, 70% within the top 3, and no confidence level reached 98% precision). Review shows its top 3 when history has nothing. Retrains nightly and after each Review session.

### 7. Decide

- A rule's set category, or a history vote ≥ 90%, auto-applies: `categorized_by = 'rule'` or `'history'`, with a reason ("rule: <name>", "9 of 10 → Groceries"). Auto-applied rows skip Review; Transactions gets an **Auto-categorized** filter for spot-checks.
- Everything else stays uncategorized and enters Review with its suggestions and the reason it's there.

### User choices win

A category a person set (`categorized_by = 'user'`) is never overwritten by rules, the pipeline, re-checks or imports. "Apply to past" touches only uncategorized and auto-applied rows.

## Rule suggestions

The app proposes a rule when:

1. the same payee is corrected to the same category **twice**;
2. a payee's categories split cleanly by amount (e.g. under $10 → Fast Food & Coffee, otherwise Gas);
3. an always-review payee gets the same category **5 times in a row** (suggest a rule plus dropping always-review);
4. History re-check **Keep** decisions split cleanly by a condition;
5. an imported Actual rule qualifies (below).

A suggestion shows as a **Make a rule?** card right after the action that triggered it, and waits in a list at the top of the Rules screen. Accepting takes one tap and opens the rule pre-filled and editable. Dismissed suggestions don't come back for the same trigger *(agent)*.

**Actual's rules** are not imported: payee history already reproduces "payee is X → category Y" rules, and old rules often contradict each other. The one exception, a general check: an Actual rule becomes a queued suggestion when its conditions are all-of a payee match (`is` / `oneOf`) plus an amount condition, its only action is set category, and that category was mapped to a real app category (not Decide later, not Transfer). Approximate amounts become a range. Rules with notes conditions or note actions never qualify. The import's Done step reads "Actual's N rules weren't copied, since payee history covers them. M rules suggested." These suggestions aren't counted on the Finish your import card.

## Review

The **Review inbox** is every uncategorized transaction on a budgeted account that isn't a transfer leg or Transfer candidate, isn't waiting for a transfer partner, isn't quiet, and isn't an Actual "Decide later" row (the `v_review_inbox` view). Tracking-account rows never enter Review *(agent: their activity never counts toward budgets)*. Oldest first. The Review tab shows the count as a badge.

**Layout: one transaction at a time**, same on phone and desktop (desktop centres the card *(agent)*):

- A big card: payee, raw description, date, account, amount, and why it's here ("always-review payee", "mixed history: 6 Groceries, 3 Household", "new payee").
- The top suggestions as large buttons with their confidence; **Other category** opens a searchable picker.
- **Split**, **Transfer** (pick the other account), **Skip** (to the back of the queue for this session only).
- **Always review <payee>** checkbox.
- A "Same as <payee>? Merge" prompt when the normalizer found a look-alike.
- A progress bar, and **Undo** for the last action.
- After a pick that triggers a rule suggestion, the **Make a rule?** card appears before the next transaction.

**Split sheet** (bottom sheet on phone, dialog on desktop): lines of category + amount, a **Rest** button filling the remainder into a line, a running **Remaining** total; Save stays disabled until the lines sum to the transaction amount.

**Uncategorized** rows count toward spending as a temporary "Uncategorized" line, so totals never look better than reality.

## Transactions: editing arrives

Transactions become editable: category (or split), payee (rename or pick another), memo, tags, exclude from budget. Every category change by a person sets `categorized_by = 'user'` and records a `categorization_feedback` row. New filters: **Auto-categorized**, **Imported, uncategorized**.

## Transfers screen

Under Money (desktop sidebar) and More (phone). Sections: **Transfer candidates** (suggested pairs, each with Confirm / Not a transfer), **Lone legs** (Pick a category / Transfer to a balances-only account), and recent matched transfers with **Unmatch** *(agent)*.

## Rules & payees

**Settings › Rules & payees.** Suggested rules first (Accept / Dismiss), then rules (name, conditions summary, actions, enabled toggle; edit, delete), then payees (search; review setting; merge; aliases).

## History re-check

Proposes fixes for past categories that disagree with a payee's strong majority. Runs after the Actual import and from a **Re-check history** button under More. **Nothing changes without approval.**

- Computed in app categories (after the Actual mapping), so the mapping decides what gets flagged. Undecided rows don't vote; Transfer-marked rows are excluded.
- A payee qualifies with **≥ 5** categorized rows and a top category holding **≥ 70%**. Rows in other categories are flagged, except a run of **≥ 3** most-recent rows in one new category, which counts as a deliberate switch.
- Rows the user kept are never flagged again (`recheck_keeps`).

**Screen: grouped by payee.** One card per payee: "Usually Gas (47 of 49). 2 are labelled otherwise." with **Fix all → Gas**, **Keep all**, **One by one** (expands rows with date, amount, current → proposed, and Fix / Keep each). A **history** link opens the payee's transactions with the flagged rows highlighted. Fixes set `categorized_by = 'user'`. When kept rows split cleanly by a condition, the card offers **Make a rule?**.

**Import Done step** gains the re-check result: "N likely slips across M payees" with **Review fixes** / **Later**.

## Accuracy

**More › Categorizer accuracy.** Last 90 days: rows auto-applied; the share of auto-applied rows later changed, by layer (rule / history); how often the top Review suggestion was picked (the classifier's report card). A warning shows when the changed share is above 2%. Below that, an all-time chart of overall accuracy by month. Nothing self-adjusts: thresholds change only in code.
