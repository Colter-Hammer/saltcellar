//! An Actual export's rows, as the importer reads them
//! (docs/spec/m1-ledger-import.md#reading-the-export).
//!
//! The `actual` crate copies Actual's raw tables into a [`Raw`] as they are; [`Raw::read`] applies
//! Actual's rules to them (tombstones, merged categories and payees, signed cents, `YYYYMMDD`
//! dates, transfer payees and twins, splits, starting balances) and keeps only what's alive.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{Date, Money};

/// A row of Actual's `accounts`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawAccount {
    pub id: String,
    pub name: String,
    pub off_budget: bool,
    pub closed: bool,
    /// Actual's bank-sync account id (`account_id`), when it has one.
    pub bank_id: Option<String>,
    pub sort_order: f64,
    pub tombstone: bool,
}

/// A row of Actual's `category_groups`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawGroup {
    pub id: String,
    pub name: String,
    pub income: bool,
    pub sort_order: f64,
    pub tombstone: bool,
}

/// A row of Actual's `categories`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawCategory {
    pub id: String,
    pub name: String,
    /// `cat_group`.
    pub group: Option<String>,
    pub sort_order: f64,
    pub tombstone: bool,
}

/// A row of Actual's `payees`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawPayee {
    pub id: String,
    pub name: Option<String>,
    /// `transfer_acct`: set when the payee is one of the household's own accounts.
    pub transfer_account: Option<String>,
    pub tombstone: bool,
}

/// A row of Actual's `transactions`.
#[expect(
    clippy::struct_excessive_bools,
    reason = "Actual's own flags, copied as they are"
)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawTransaction {
    pub id: String,
    /// `acct`.
    pub account: Option<String>,
    /// A category id, to resolve through `category_mapping`.
    pub category: Option<String>,
    /// Signed integer cents, negative for money out.
    pub amount: Option<i64>,
    /// `description`: a payee id, to resolve through `payee_mapping`.
    pub payee: Option<String>,
    /// The bank's own text.
    pub imported_description: Option<String>,
    pub notes: Option<String>,
    /// `YYYYMMDD`.
    pub date: Option<i64>,
    /// The bank's transaction id.
    pub financial_id: Option<String>,
    pub is_parent: bool,
    pub is_child: bool,
    pub parent_id: Option<String>,
    /// The other leg of a transfer.
    pub transferred_id: Option<String>,
    pub starting_balance: bool,
    pub tombstone: bool,
}

/// Actual's raw tables, the parts the importer reads, as they are in the export.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Raw {
    pub accounts: Vec<RawAccount>,
    pub groups: Vec<RawGroup>,
    pub categories: Vec<RawCategory>,
    /// `category_mapping`: `(id, transferId)`. A merged category points at the one it went into.
    pub category_mapping: Vec<(String, String)>,
    pub payees: Vec<RawPayee>,
    /// `payee_mapping`: `(id, targetId)`. A merged payee points at the one it went into.
    pub payee_mapping: Vec<(String, String)>,
    pub transactions: Vec<RawTransaction>,
}

/// What's alive in an export: its accounts, categories and transactions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    /// Open before closed, on budget before off budget, then in Actual's order *(agent)*.
    pub accounts: Vec<Account>,
    /// In Actual's order: by group, then within it. "Starting Balances" included.
    pub categories: Vec<Category>,
    /// Split children are inside their parents; Actual's order.
    pub transactions: Vec<Transaction>,
}

/// A live Actual account and its figures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub off_budget: bool,
    pub closed: bool,
    /// Actual's bank-sync account id, kept as `external_account_id`.
    pub bank_id: Option<String>,
    /// Its rows: a split counts once, a starting balance counts.
    pub rows: u64,
    /// The sum of all its rows, starting balances included: the balance Actual shows.
    pub balance: Money,
}

/// A live Actual category, in a live group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub group: String,
    pub income: bool,
    /// How many rows in each account (by Actual id) use it: plain rows and split lines, never
    /// starting balances.
    pub rows: BTreeMap<String, u64>,
}

impl Category {
    /// Whether it's Actual's own "Starting Balances" category, whose rows become opening balances.
    #[must_use]
    pub fn is_starting_balances(&self) -> bool {
        self.name.eq_ignore_ascii_case("Starting Balances")
    }
}

/// Who a row is with, once Actual's ids are resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payee {
    /// A merchant or person, by name.
    Named(String),
    /// Another of the household's accounts (by Actual id): the row is a transfer.
    Transfer(String),
}

/// A live row that isn't a split child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    pub id: String,
    /// The account's Actual id.
    pub account: String,
    pub date: Date,
    /// The total, for a split.
    pub amount: Money,
    pub payee: Option<Payee>,
    /// A live category's Actual id; `None` for no category, or one that resolves to a deleted one.
    pub category: Option<String>,
    pub imported_description: Option<String>,
    pub financial_id: Option<String>,
    pub notes: Option<String>,
    /// The other leg of a transfer (a row or a split line), when that one is alive.
    pub twin: Option<String>,
    pub starting_balance: bool,
    /// A split's live lines; empty for a row that isn't split.
    pub lines: Vec<Line>,
}

/// A live line of a split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub id: String,
    pub amount: Money,
    pub payee: Option<Payee>,
    pub category: Option<String>,
    pub notes: Option<String>,
    pub twin: Option<String>,
}

/// A value in the export the app can't read, saying which.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Unreadable(pub String);

/// Actual's integer `YYYYMMDD` date.
///
/// # Errors
/// If it isn't a day on the calendar.
pub fn parse_date(date: i64) -> Result<Date, Unreadable> {
    let no_such_day = || Unreadable(format!("a transaction dated {date}"));
    let parts = (
        i16::try_from(date / 10_000),
        i8::try_from(date / 100 % 100),
        i8::try_from(date % 100),
    );
    match parts {
        (Ok(year), Ok(month), Ok(day)) => Date::from_ymd(year, month, day).ok(),
        _ => None,
    }
    .ok_or_else(no_such_day)
}

/// `id` through a merge mapping: where it went, or itself when it was never merged.
fn through<'a>(mapping: &HashMap<&'a str, &'a str>, id: &'a str) -> &'a str {
    mapping.get(id).copied().unwrap_or(id)
}

fn mapping(pairs: &[(String, String)]) -> HashMap<&str, &str> {
    pairs
        .iter()
        .map(|(id, target)| (id.as_str(), target.as_str()))
        .collect()
}

/// Lookups shared by every row: what's alive, and where merged ids went.
struct Resolver<'a> {
    accounts: HashSet<&'a str>,
    categories: HashSet<&'a str>,
    category_mapping: HashMap<&'a str, &'a str>,
    payees: HashMap<&'a str, &'a RawPayee>,
    payee_mapping: HashMap<&'a str, &'a str>,
}

impl<'a> Resolver<'a> {
    /// A live category's id, or `None`: no category, or one that resolves to a deleted one.
    fn category(&self, id: Option<&'a str>) -> Option<String> {
        let target = through(&self.category_mapping, id?);
        self.categories.contains(target).then(|| target.to_owned())
    }

    /// The payee, or `None`: no payee, a deleted or nameless one, or a transfer to a deleted
    /// account.
    fn payee(&self, id: Option<&'a str>) -> Option<Payee> {
        let payee = self.payees.get(through(&self.payee_mapping, id?))?;
        match &payee.transfer_account {
            Some(account) => self
                .accounts
                .contains(account.as_str())
                .then(|| Payee::Transfer(account.clone())),
            None => payee
                .name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(|name| Payee::Named(name.to_owned())),
        }
    }
}

impl Raw {
    /// The live accounts, categories and transactions, with each account's figures and each
    /// category's use.
    ///
    /// # Errors
    /// [`Unreadable`] for a date that isn't on the calendar, or a balance too large to add up.
    pub fn read(&self) -> Result<Export, Unreadable> {
        let mut live_accounts: Vec<&RawAccount> =
            self.accounts.iter().filter(|a| !a.tombstone).collect();
        live_accounts.sort_by(|a, b| {
            (a.closed, a.off_budget)
                .cmp(&(b.closed, b.off_budget))
                .then(a.sort_order.total_cmp(&b.sort_order))
        });
        let groups: HashMap<&str, &RawGroup> = self
            .groups
            .iter()
            .filter(|group| !group.tombstone)
            .map(|group| (group.id.as_str(), group))
            .collect();
        // A category is alive only in a live group *(agent)*.
        let mut live_categories: Vec<(&RawCategory, &RawGroup)> = self
            .categories
            .iter()
            .filter(|category| !category.tombstone)
            .filter_map(|category| Some((category, *groups.get(category.group.as_deref()?)?)))
            .collect();
        live_categories.sort_by(|(a, a_group), (b, b_group)| {
            order(a_group.sort_order, b_group.sort_order)
                .then(a_group.id.cmp(&b_group.id))
                .then(order(a.sort_order, b.sort_order))
        });
        let resolver = Resolver {
            accounts: live_accounts.iter().map(|a| a.id.as_str()).collect(),
            categories: live_categories.iter().map(|(c, _)| c.id.as_str()).collect(),
            category_mapping: mapping(&self.category_mapping),
            payees: self
                .payees
                .iter()
                .filter(|payee| !payee.tombstone)
                .map(|payee| (payee.id.as_str(), payee))
                .collect(),
            payee_mapping: mapping(&self.payee_mapping),
        };

        let transactions = self.live_transactions(&resolver)?;
        let mut accounts = Vec::with_capacity(live_accounts.len());
        for account in live_accounts {
            let rows: Vec<&Transaction> = transactions
                .iter()
                .filter(|t| t.account == account.id)
                .collect();
            let balance = Money::checked_sum(rows.iter().map(|t| t.amount))
                .ok_or_else(|| Unreadable(format!("the balance of {}", account.name)))?;
            accounts.push(Account {
                id: account.id.clone(),
                name: account.name.clone(),
                off_budget: account.off_budget,
                closed: account.closed,
                bank_id: account.bank_id.clone(),
                rows: rows.len() as u64,
                balance,
            });
        }

        let mut used: HashMap<&str, BTreeMap<String, u64>> = HashMap::new();
        for transaction in transactions.iter().filter(|t| !t.starting_balance) {
            let categories: Vec<&Option<String>> = if transaction.lines.is_empty() {
                vec![&transaction.category]
            } else {
                transaction
                    .lines
                    .iter()
                    .map(|line| &line.category)
                    .collect()
            };
            for category in categories.into_iter().flatten() {
                *used
                    .entry(category)
                    .or_default()
                    .entry(transaction.account.clone())
                    .or_default() += 1;
            }
        }
        let categories = live_categories
            .into_iter()
            .map(|(category, group)| Category {
                id: category.id.clone(),
                name: category.name.clone(),
                group: group.name.clone(),
                income: group.income,
                rows: used.remove(category.id.as_str()).unwrap_or_default(),
            })
            .collect();

        Ok(Export {
            accounts,
            categories,
            transactions,
        })
    }

    /// Every live row, split children inside their parents. A row is alive when it isn't
    /// tombstoned, has a date and is in a live account; a split child only when its parent is too.
    fn live_transactions(&self, resolver: &Resolver<'_>) -> Result<Vec<Transaction>, Unreadable> {
        let by_id: HashMap<&str, &RawTransaction> = self
            .transactions
            .iter()
            .map(|transaction| (transaction.id.as_str(), transaction))
            .collect();
        let row_alive = |t: &RawTransaction| {
            !t.tombstone
                && t.date.is_some()
                && t.account
                    .as_deref()
                    .is_some_and(|id| resolver.accounts.contains(id))
        };
        let parent = |t: &RawTransaction| t.parent_id.as_deref().and_then(|id| by_id.get(id));
        let alive = |t: &RawTransaction| {
            row_alive(t) && (!t.is_child || parent(t).is_some_and(|p| !p.is_child && row_alive(p)))
        };
        let alive_ids: HashSet<&str> = self
            .transactions
            .iter()
            .filter(|t| alive(t))
            .map(|t| t.id.as_str())
            .collect();
        let twin = |t: &RawTransaction| {
            t.transferred_id
                .clone()
                .filter(|id| alive_ids.contains(id.as_str()))
        };
        let amount = |t: &RawTransaction| Money::from_minor(t.amount.unwrap_or(0));

        let mut lines: HashMap<&str, Vec<Line>> = HashMap::new();
        let mut rows = Vec::new();
        for t in self.transactions.iter().filter(|t| alive(t)) {
            if t.is_child {
                let parent = t.parent_id.as_deref().unwrap_or_default();
                lines.entry(parent).or_default().push(Line {
                    id: t.id.clone(),
                    amount: amount(t),
                    payee: resolver.payee(t.payee.as_deref()),
                    category: resolver.category(t.category.as_deref()),
                    notes: t.notes.clone(),
                    twin: twin(t),
                });
                continue;
            }
            rows.push(Transaction {
                id: t.id.clone(),
                account: t.account.clone().unwrap_or_default(),
                date: parse_date(t.date.unwrap_or_default())?,
                amount: amount(t),
                payee: resolver.payee(t.payee.as_deref()),
                category: resolver.category(t.category.as_deref()),
                imported_description: t.imported_description.clone(),
                financial_id: t.financial_id.clone(),
                notes: t.notes.clone(),
                twin: twin(t),
                starting_balance: t.starting_balance,
                lines: Vec::new(),
            });
        }
        for row in &mut rows {
            row.lines = lines.remove(row.id.as_str()).unwrap_or_default();
        }
        Ok(rows)
    }
}

fn order(a: f64, b: f64) -> Ordering {
    a.total_cmp(&b)
}

impl Export {
    /// The earliest and latest dates of its rows; `None` when there are none.
    #[must_use]
    pub fn dates(&self) -> Option<(Date, Date)> {
        let first = self.transactions.iter().map(|t| t.date).min()?;
        let last = self.transactions.iter().map(|t| t.date).max()?;
        Some((first, last))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str) -> RawAccount {
        RawAccount {
            id: id.to_owned(),
            name: format!("Account {id}"),
            ..RawAccount::default()
        }
    }

    fn group(id: &str) -> RawGroup {
        RawGroup {
            id: id.to_owned(),
            name: format!("Group {id}"),
            ..RawGroup::default()
        }
    }

    fn category(id: &str, group: &str) -> RawCategory {
        RawCategory {
            id: id.to_owned(),
            name: format!("Category {id}"),
            group: Some(group.to_owned()),
            ..RawCategory::default()
        }
    }

    fn payee(id: &str, name: &str) -> RawPayee {
        RawPayee {
            id: id.to_owned(),
            name: Some(name.to_owned()),
            ..RawPayee::default()
        }
    }

    fn row(id: &str, account: &str, amount: i64) -> RawTransaction {
        RawTransaction {
            id: id.to_owned(),
            account: Some(account.to_owned()),
            amount: Some(amount),
            date: Some(20_250_301),
            ..RawTransaction::default()
        }
    }

    /// One live account `a`, group `g` with categories `c1` and `c2`, and payee `p`.
    fn household() -> Raw {
        Raw {
            accounts: vec![account("a")],
            groups: vec![group("g")],
            categories: vec![category("c1", "g"), category("c2", "g")],
            payees: vec![payee("p", "Corner Grocer")],
            ..Raw::default()
        }
    }

    fn ids(export: &Export) -> Vec<&str> {
        export.transactions.iter().map(|t| t.id.as_str()).collect()
    }

    fn only(export: &Export) -> &Transaction {
        assert_eq!(export.transactions.len(), 1, "{:?}", ids(export));
        &export.transactions[0]
    }

    // ── Tombstones ──

    #[test]
    fn tombstoned_rows_are_skipped_in_every_table() {
        let mut raw = household();
        raw.accounts.push(RawAccount {
            tombstone: true,
            ..account("gone")
        });
        raw.groups.push(RawGroup {
            tombstone: true,
            ..group("old")
        });
        raw.categories.push(RawCategory {
            tombstone: true,
            ..category("c3", "g")
        });
        raw.categories.push(category("c4", "old"));
        raw.payees.push(RawPayee {
            tombstone: true,
            ..payee("q", "Old Video Store")
        });
        raw.transactions = vec![
            row("t1", "a", -100),
            RawTransaction {
                tombstone: true,
                ..row("t2", "a", -200)
            },
            row("t3", "gone", -300),
            RawTransaction {
                category: Some("c3".to_owned()),
                payee: Some("q".to_owned()),
                ..row("t4", "a", -400)
            },
        ];
        let export = raw.read().unwrap();
        assert_eq!(
            export
                .accounts
                .iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>(),
            ["a"]
        );
        // c4 is live itself, but in a deleted group.
        assert_eq!(
            export
                .categories
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            ["c1", "c2"]
        );
        assert_eq!(ids(&export), ["t1", "t4"]);
        // A deleted payee and a deleted category leave the row without either.
        assert_eq!(export.transactions[1].payee, None);
        assert_eq!(export.transactions[1].category, None);
    }

    #[test]
    fn a_split_child_is_alive_only_if_it_and_its_parent_are() {
        let mut raw = household();
        let parent = |id: &str, tombstone| RawTransaction {
            is_parent: true,
            tombstone,
            ..row(id, "a", -300)
        };
        let child = |id: &str, parent: Option<&str>, tombstone| RawTransaction {
            is_child: true,
            parent_id: parent.map(str::to_owned),
            tombstone,
            ..row(id, "a", -100)
        };
        raw.transactions = vec![
            parent("live", false),
            child("live/1", Some("live"), false),
            child("live/2", Some("live"), true),
            parent("dead", true),
            child("dead/1", Some("dead"), false),
            // No parent at all, a parent that isn't in the export, and a child as a parent.
            child("orphan", None, false),
            child("lost", Some("nowhere"), false),
            child("nested", Some("live/1"), false),
        ];
        let export = raw.read().unwrap();
        assert_eq!(ids(&export), ["live"]);
        let lines: Vec<&str> = only(&export).lines.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(lines, ["live/1"]);
    }

    #[test]
    fn rows_without_a_date_or_an_account_are_left_out() {
        let mut raw = household();
        raw.transactions = vec![
            RawTransaction {
                date: None,
                ..row("undated", "a", -100)
            },
            RawTransaction {
                account: None,
                ..row("nowhere", "a", -100)
            },
        ];
        assert_eq!(ids(&raw.read().unwrap()), Vec::<&str>::new());
    }

    // ── Id indirection ──

    #[test]
    fn a_category_resolves_through_category_mapping() {
        let mut raw = household();
        raw.categories.push(RawCategory {
            tombstone: true,
            ..category("merged", "g")
        });
        raw.categories.push(RawCategory {
            tombstone: true,
            ..category("deleted", "g")
        });
        raw.category_mapping = vec![
            ("merged".to_owned(), "c2".to_owned()),
            ("c1".to_owned(), "c1".to_owned()),
            ("into-deleted".to_owned(), "deleted".to_owned()),
        ];
        let with = |id: &str, category: &str| RawTransaction {
            category: Some(category.to_owned()),
            ..row(id, "a", -100)
        };
        raw.transactions = vec![
            with("t1", "merged"),
            with("t2", "c1"),
            // Not in the mapping: the id itself.
            with("t3", "c2"),
            // Resolves to a deleted category, or to nothing: uncategorized.
            with("t4", "into-deleted"),
            with("t5", "deleted"),
            with("t6", "unknown"),
            row("t7", "a", -100),
        ];
        let export = raw.read().unwrap();
        let categories: Vec<Option<&str>> = export
            .transactions
            .iter()
            .map(|t| t.category.as_deref())
            .collect();
        assert_eq!(
            categories,
            [Some("c2"), Some("c1"), Some("c2"), None, None, None, None]
        );
    }

    #[test]
    fn the_description_is_a_payee_id_resolving_through_payee_mapping() {
        let mut raw = household();
        raw.payees.push(RawPayee {
            tombstone: true,
            ..payee("merged", "Corner Grocery 12")
        });
        raw.payees.push(RawPayee {
            name: None,
            ..payee("nameless", "")
        });
        raw.payees.push(payee("blank", "  "));
        raw.payee_mapping = vec![("merged".to_owned(), "p".to_owned())];
        let with = |id: &str, payee: &str| RawTransaction {
            payee: Some(payee.to_owned()),
            ..row(id, "a", -100)
        };
        raw.transactions = vec![
            with("t1", "merged"),
            with("t2", "p"),
            with("t3", "nameless"),
            with("t4", "blank"),
            with("t5", "unknown"),
            row("t6", "a", -100),
        ];
        let export = raw.read().unwrap();
        let grocer = Some(Payee::Named("Corner Grocer".to_owned()));
        let payees: Vec<Option<Payee>> = export
            .transactions
            .iter()
            .map(|t| t.payee.clone())
            .collect();
        assert_eq!(payees, [grocer.clone(), grocer, None, None, None, None]);
    }

    // ── Formats ──

    #[test]
    fn amounts_are_signed_cents_and_null_is_zero() {
        let mut raw = household();
        raw.transactions = vec![
            row("out", "a", -1_250),
            row("in", "a", 320_000),
            RawTransaction {
                amount: None,
                ..row("null", "a", 0)
            },
        ];
        let export = raw.read().unwrap();
        let amounts: Vec<i64> = export
            .transactions
            .iter()
            .map(|t| t.amount.minor())
            .collect();
        assert_eq!(amounts, [-1_250, 320_000, 0]);
    }

    #[test]
    fn dates_are_yyyymmdd_integers() {
        assert_eq!(parse_date(20_260_131).unwrap().to_string(), "2026-01-31");
        for bad in [20_260_231, 20_261_301, 0, -20_260_101, 99_999_999_999] {
            assert_eq!(
                parse_date(bad).unwrap_err(),
                Unreadable(format!("a transaction dated {bad}"))
            );
        }
        let mut raw = household();
        raw.transactions = vec![RawTransaction {
            date: Some(20_250_230),
            ..row("t1", "a", -100)
        }];
        assert_eq!(
            raw.read().unwrap_err().to_string(),
            "a transaction dated 20250230"
        );
    }

    #[test]
    fn the_dates_span_the_rows() {
        let mut raw = household();
        assert_eq!(raw.read().unwrap().dates(), None);
        raw.transactions = vec![
            RawTransaction {
                date: Some(20_250_715),
                ..row("t1", "a", -100)
            },
            RawTransaction {
                date: Some(20_250_102),
                ..row("t2", "a", -100)
            },
            RawTransaction {
                date: Some(20_260_105),
                ..row("t3", "a", -100)
            },
        ];
        let (first, last) = raw.read().unwrap().dates().unwrap();
        assert_eq!(
            (first.to_string(), last.to_string()),
            ("2025-01-02".to_owned(), "2026-01-05".to_owned())
        );
    }

    // ── Bank text ──

    #[test]
    fn bank_text_id_and_notes_come_through() {
        let mut raw = household();
        raw.transactions = vec![RawTransaction {
            imported_description: Some("CORNER GROCER 0042".to_owned()),
            financial_id: Some("fit-1".to_owned()),
            notes: Some("Big shop #household".to_owned()),
            ..row("t1", "a", -100)
        }];
        let export = raw.read().unwrap();
        let t = only(&export);
        assert_eq!(
            t.imported_description.as_deref(),
            Some("CORNER GROCER 0042")
        );
        assert_eq!(t.financial_id.as_deref(), Some("fit-1"));
        assert_eq!(t.notes.as_deref(), Some("Big shop #household"));
        assert_eq!(t.date.to_string(), "2025-03-01");
    }

    // ── Transfers ──

    #[test]
    fn a_transfer_payee_is_an_account_and_twins_link_through_transferred_id() {
        let mut raw = household();
        raw.accounts.push(account("b"));
        raw.accounts.push(RawAccount {
            tombstone: true,
            ..account("gone")
        });
        let to = |id: &str, account: &str| RawPayee {
            transfer_account: Some(account.to_owned()),
            ..payee(id, "")
        };
        raw.payees
            .extend([to("to-a", "a"), to("to-b", "b"), to("to-gone", "gone")]);
        raw.transactions = vec![
            RawTransaction {
                payee: Some("to-b".to_owned()),
                transferred_id: Some("in".to_owned()),
                ..row("out", "a", -500)
            },
            RawTransaction {
                payee: Some("to-a".to_owned()),
                transferred_id: Some("out".to_owned()),
                ..row("in", "b", 500)
            },
            // Its twin was deleted, and so was the account on the other side.
            RawTransaction {
                payee: Some("to-gone".to_owned()),
                transferred_id: Some("deleted".to_owned()),
                ..row("lone", "a", -700)
            },
            RawTransaction {
                tombstone: true,
                ..row("deleted", "gone", 700)
            },
        ];
        let export = raw.read().unwrap();
        let legs: Vec<(Option<Payee>, Option<&str>)> = export
            .transactions
            .iter()
            .map(|t| (t.payee.clone(), t.twin.as_deref()))
            .collect();
        assert_eq!(
            legs,
            [
                (Some(Payee::Transfer("b".to_owned())), Some("in")),
                (Some(Payee::Transfer("a".to_owned())), Some("out")),
                (None, None),
            ]
        );
    }

    // ── Splits ──

    #[test]
    fn a_split_is_its_parent_holding_the_total_with_its_lines_never_summed_twice() {
        let mut raw = household();
        raw.payees.push(RawPayee {
            transfer_account: Some("a".to_owned()),
            ..payee("to-a", "")
        });
        let child = |id: &str, category: &str, amount| RawTransaction {
            is_child: true,
            parent_id: Some("split".to_owned()),
            category: Some(category.to_owned()),
            payee: Some("p".to_owned()),
            notes: Some(format!("line {id}")),
            ..row(id, "a", amount)
        };
        raw.transactions = vec![
            RawTransaction {
                is_parent: true,
                payee: Some("p".to_owned()),
                ..row("split", "a", -12_000)
            },
            child("s1", "c1", -8_000),
            child("s2", "c2", -4_000),
            RawTransaction {
                transferred_id: Some("s1".to_owned()),
                ..row("twin", "a", 0)
            },
        ];
        let export = raw.read().unwrap();
        assert_eq!(ids(&export), ["split", "twin"]);
        let split = &export.transactions[0];
        assert_eq!(split.amount, Money::from_minor(-12_000));
        assert_eq!(split.category, None);
        assert_eq!(
            split.lines,
            [
                Line {
                    id: "s1".to_owned(),
                    amount: Money::from_minor(-8_000),
                    payee: Some(Payee::Named("Corner Grocer".to_owned())),
                    category: Some("c1".to_owned()),
                    notes: Some("line s1".to_owned()),
                    twin: None,
                },
                Line {
                    id: "s2".to_owned(),
                    amount: Money::from_minor(-4_000),
                    payee: Some(Payee::Named("Corner Grocer".to_owned())),
                    category: Some("c2".to_owned()),
                    notes: Some("line s2".to_owned()),
                    twin: None,
                },
            ]
        );
        // A twin can be a split line.
        assert_eq!(export.transactions[1].twin.as_deref(), Some("s1"));
        // The balance counts the parent, not its lines as well.
        assert_eq!(export.accounts[0].balance, Money::from_minor(-12_000));
        assert_eq!(export.accounts[0].rows, 2);
    }

    // ── Starting balances ──

    #[test]
    fn starting_balance_rows_count_in_the_balance_but_use_no_category() {
        let mut raw = household();
        raw.categories.push(RawCategory {
            name: "Starting Balances".to_owned(),
            ..category("start", "g")
        });
        raw.transactions = vec![
            RawTransaction {
                starting_balance: true,
                category: Some("start".to_owned()),
                ..row("t1", "a", 250_000)
            },
            RawTransaction {
                category: Some("c1".to_owned()),
                ..row("t2", "a", -1_000)
            },
        ];
        let export = raw.read().unwrap();
        assert!(export.transactions[0].starting_balance);
        assert!(!export.transactions[1].starting_balance);
        assert_eq!(export.accounts[0].rows, 2);
        assert_eq!(export.accounts[0].balance, Money::from_minor(249_000));
        let start = export.categories.iter().find(|c| c.id == "start").unwrap();
        assert!(start.is_starting_balances());
        assert!(start.rows.is_empty());
        assert!(!export.categories[0].is_starting_balances());
    }

    // ── Figures ──

    #[test]
    fn each_account_has_its_rows_and_balance() {
        let mut raw = household();
        raw.accounts = vec![
            RawAccount {
                name: "Old checking".to_owned(),
                closed: true,
                sort_order: 0.0,
                ..account("closed")
            },
            RawAccount {
                name: "Brokerage".to_owned(),
                off_budget: true,
                bank_id: Some("bank-1".to_owned()),
                sort_order: 1.0,
                ..account("off")
            },
            RawAccount {
                sort_order: 3.0,
                ..account("b")
            },
            RawAccount {
                sort_order: 2.0,
                ..account("a")
            },
        ];
        raw.transactions = vec![
            row("t1", "a", -100),
            row("t2", "a", -250),
            row("t3", "off", 1_000),
        ];
        let export = raw.read().unwrap();
        let figures: Vec<(&str, bool, bool, u64, i64)> = export
            .accounts
            .iter()
            .map(|a| {
                (
                    a.id.as_str(),
                    a.off_budget,
                    a.closed,
                    a.rows,
                    a.balance.minor(),
                )
            })
            .collect();
        assert_eq!(
            figures,
            [
                ("a", false, false, 2, -350),
                ("b", false, false, 0, 0),
                ("off", true, false, 1, 1_000),
                ("closed", false, true, 0, 0),
            ]
        );
        assert_eq!(export.accounts[2].name, "Brokerage");
        assert_eq!(export.accounts[2].bank_id.as_deref(), Some("bank-1"));
    }

    #[test]
    fn a_balance_too_large_to_add_up_is_unreadable() {
        let mut raw = household();
        raw.transactions = vec![row("t1", "a", i64::MAX), row("t2", "a", 1)];
        assert_eq!(
            raw.read().unwrap_err().to_string(),
            "the balance of Account a"
        );
    }

    /// A category's id, group, income flag and rows per account.
    type Use<'a> = (&'a str, &'a str, bool, Vec<(&'a str, u64)>);

    #[test]
    fn each_category_counts_the_rows_using_it_per_account() {
        let mut raw = household();
        raw.accounts.push(account("b"));
        raw.groups = vec![
            RawGroup {
                sort_order: 2.0,
                ..group("g")
            },
            RawGroup {
                name: "Income".to_owned(),
                income: true,
                sort_order: 1.0,
                ..group("income")
            },
        ];
        raw.categories = vec![
            RawCategory {
                sort_order: 2.0,
                ..category("c1", "g")
            },
            RawCategory {
                sort_order: 1.0,
                ..category("c2", "g")
            },
            category("pay", "income"),
            category("unused", "g"),
        ];
        let with = |id: &str, account: &str, category: &str| RawTransaction {
            category: Some(category.to_owned()),
            ..row(id, account, -100)
        };
        raw.transactions = vec![
            with("t1", "a", "c1"),
            with("t2", "a", "c1"),
            with("t3", "b", "c1"),
            with("t4", "b", "pay"),
            RawTransaction {
                is_parent: true,
                ..row("split", "b", -200)
            },
            RawTransaction {
                is_child: true,
                parent_id: Some("split".to_owned()),
                ..with("s1", "b", "c2")
            },
            RawTransaction {
                is_child: true,
                parent_id: Some("split".to_owned()),
                ..row("s2", "b", -100)
            },
        ];
        let export = raw.read().unwrap();
        let uses: Vec<Use> = export
            .categories
            .iter()
            .map(|c| {
                (
                    c.id.as_str(),
                    c.group.as_str(),
                    c.income,
                    c.rows.iter().map(|(a, n)| (a.as_str(), *n)).collect(),
                )
            })
            .collect();
        assert_eq!(
            uses,
            [
                ("pay", "Income", true, vec![("b", 1)]),
                ("unused", "Group g", false, vec![]),
                ("c2", "Group g", false, vec![("b", 1)]),
                ("c1", "Group g", false, vec![("a", 2), ("b", 1)]),
            ]
        );
        assert_eq!(export.categories[3].name, "Category c1");
    }
}
