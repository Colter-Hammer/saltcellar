# Milestone 5: Funds + savings goals

**Goal:** money set aside for later is visible: play money per person, sinking funds for known expenses, and goals for savings accounts.

**Done when:** play money and sinking funds can be created, edited and seen on the Funds screen and Overview, savings goals show progress, and the Playwright smoke test covers creating a play-money fund and seeing its balance move.

## Funds

A **fund** is a running balance that builds up from contributions and is spent down by spending in its category, on any account, carrying forward across periods. It is the deliberate exception to "nothing carries over" ([ADR-0003](../adr/0003-funds-carry-over.md)). Two kinds:

- **Play money:** belongs to one login; a fixed monthly contribution, spendable on anything, accumulating over time. Its spending counts as one line in that person's play-money category, never in ordinary categories. Setting one up creates the person's category (in a **Play Money** group, created if missing).
- **Sinking fund:** attached to a budget; its monthly contribution is the budget's target spread across the window, and leftover money carries forward instead of resetting. Spreading: target ÷ months, with the remainder in the window's last month *(agent)*.

**Balance** = starting balance + contributions + adjustments − spending in the fund's category since the start month. Always computed, never read from an account.

- **On paper:** contributions follow the schedule (play money's monthly amount, effective from a month forward; a sinking fund's spread target).
- **Backed by an account:** the account must be **budgeted** (otherwise spending inside it would never count); tracking accounts can't back a fund. Contributions are the actual transfers into that account; spending is still the fund's category on any account. The account's real balance is shown beside the fund's, and the gap explained ("$40 still to move", e.g. to pay the card a play-money purchase went on).
- **Adjustments** correct a paper fund by hand, with a note.

### Creating and editing

- **Create** is a 4-step guided flow: kind (play money / sinking fund) → owner and amount → where the money is (on paper, or a budgeted account) → starting balance. Choosing "saving toward a budget" goes to Budget setup with **Save for this** on (which creates the sinking fund).
- **Edit** is a form with the two balances (fund vs. backing account, with the gap explained); a fund's detail view is its month-by-month ledger (contributions, spending, adjustments, running balance). A sinking fund's amount is edited through its budget.

### Funds screen

Under Money (desktop) and More (phone): a card per fund with its balance, this month's contribution and a sparkline; tap → detail. Overview's **Funds** card shows the balances.

## Savings goals

A **savings goal** is a target for one or more **tracking** accounts. Two kinds:

- **Contribution goal:** an amount to move in per period of N months (under target is bad). Progress = net money moved into the goal's accounts in the current period (transfers in minus transfers out) *(agent)*.
- **Balance goal:** an amount to hold, either **by a date** or **ongoing**. Progress = the sum of the accounts' latest balances. An ongoing goal is met while the balance is at or above target, and becomes unmet again if money is withdrawn.

An emergency fund is a balance goal on a tracking account, not a fund.

**Savings goals screen** (Money / More): goal cards with progress, plus each tracking account's balance over time. **Settings › Savings goals** holds the form (name, kind, target, period or date, accounts).
