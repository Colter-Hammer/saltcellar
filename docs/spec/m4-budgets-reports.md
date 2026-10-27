# Milestone 4: Budgets, Overview, reports

**Goal:** set targets, see at a glance how the month is going, and answer "where does the money go?". Priority among reports: spending by group (with drill-down), budget vs. actual, then cash flow; trends after. All of them work on a phone.

**Done when:** budgets can be set up and edited, the Budget screen and Overview show real numbers, the four reports work on phone and desktop, Overview becomes the landing screen, and the Playwright smoke test covers setting a budget and seeing it on Overview.

## Budgets

A **budget** is a spending target for a category or a category group over a period, compared with actual spending. Tracking-style: nothing carries over (funds, in milestone 5, are the exception).

- **Group budget** = an overall cap. **Category budget** = a limit within its group's cap. Warn (don't block) when a group's monthly category targets add up to more than the group's target.
- **Period:** monthly, or fixed windows of N months anchored at a chosen start month (e.g. a quarterly water bill: Feb–Apr, May–Jul…; car insurance every 6 months from July).
- A category budget on a **multi-month window** is judged against its own window only and **doesn't count toward its group's monthly cap**: the group's monthly spending leaves out categories budgeted over multi-month windows *(agent: the interpretation for spending; the agreed rule is about the cap)*.
- **Repeating** (default) or **one-time** (exactly one window, e.g. new tires).
- **Target changes apply forward** from the window chosen; earlier windows keep their old target. A **one-off override** changes a single window ("December: Gifts $600").
- Budgets are on expense groups and categories only *(agent)*.

**What counts as spent** in a window: the category's lines from `v_category_lines` dated inside the window, sign flipped so spending is positive. Pending rows count right away; refunds reduce spending; split lines count in their own categories; transfer legs and Transfer candidates don't count; uncategorized rows show as a temporary **Uncategorized** line.

**Pace.** Each bar carries a "today" tick at the share of its window that has passed (in the household timezone). Status: **Over** (spent > target), **Ahead of pace** (spent share of target is more than 10 points ahead of the elapsed share *(agent)*), **On track**.

### Budget setup

A form (sheet on phone, dialog on desktop), opened from the Budget screen or **Settings › Budgets**: what it's for (a group or a category), amount, period (monthly / every N months), first window's start month, repeating or one-time. Editing asks **From now on** or **This window only**. A side panel shows the group's cap against its category limits. The **Save for this** toggle (turns the budget into a sinking fund) appears in milestone 5.

### Budget screen

Two views of the same screen, switchable, **nested bars by default**:

- **Nested bars:** each group with its cap bar, category limit bars nested beneath, multi-month budgets labelled with their window.
- **Sorted by risk:** a flat list in three sections, Over / Ahead of pace / On track, with a strip of group bars on top.

A month picker moves between windows, so the same screen is the budget-vs-actual history *(agent)*. Tapping a row opens its edit sheet; tapping the spent amount opens Transactions filtered to that category and window.

## Overview

The landing screen from this milestone on. **Glance cards**, each linking to its screen:

- **This month:** spent vs. the month's total budget, with the pace tick.
- **Review:** count waiting, with **Start review**.
- **Groups:** a bar per budgeted group.
- **Cash flow:** a small chart of the last 6 months *(agent: 6)*.
- **Funds:** balances (from milestone 5).
- **Get started** checklist while setup is incomplete: add accounts (bank connection, file import or manual), use starter categories or import from Actual, set budgets, invite people (from milestone 6). Each item ticks itself off when done; the card can be dismissed *(agent)*.
- **Finish your import** while there's anything left from the Actual import: undecided rows, re-check fixes waiting, and imported uncategorized rows. Disappears when all are empty.

## Reports

Under Reports in the desktop sidebar; on phone under More. Each has a period picker (this month, last month, last 3/6/12 months, this year, custom) *(agent)*.

- **Spending:** a donut plus a ranked list by group; tap a group → its categories → their transactions. Uncategorized shows as its own slice.
- **Budget vs. actual:** the Budget screen with its month picker (above).
- **Trends:** pick groups; 12-month lines with each group's average as a dashed line; a table of each month against the average.
- **Cash flow:** monthly bars for **Income**, **Spending** and **Saved**, with a **Left over** line, and a monthly breakdown table.

**Cash flow definitions** (budgeted accounts only):

- **Income** = lines in income-group categories.
- **Spending** = lines in expense-group categories plus Uncategorized, sign flipped.
- **Saved** = net money moved from budgeted into tracking accounts (`v_saved_lines`); money coming back out is negative Saved, never income.
- **Left over** = Income − Spending − Saved.

Keep the data these reports read; later reports (net worth, sinking fund progress, play money remaining, merchants, budget history timeline, drift) build on it.
