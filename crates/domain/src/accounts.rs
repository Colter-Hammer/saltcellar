//! Accounts: their type and role, and their balances (docs/spec/m1-ledger-import.md#accounts).

use crate::{Date, Money};

/// What kind of account it is, as the household would call it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccountType {
    Checking,
    Savings,
    CreditCard,
    Investment,
    Loan,
    Cash,
    Other,
}

/// Words in an account's name that say its type, checked in this order so "card" beats "cash" in
/// "Cash back card" *(agent)*. Matched as whole words, case-insensitively.
const TYPE_WORDS: &[(AccountType, &[&str])] = &[
    (
        AccountType::CreditCard,
        &["card", "credit", "visa", "mastercard", "amex"],
    ),
    (AccountType::Loan, &["loan", "mortgage", "heloc"]),
    (
        AccountType::Investment,
        &[
            "brokerage",
            "invest",
            "investment",
            "investments",
            "401k",
            "403b",
            "ira",
            "roth",
            "hsa",
            "retirement",
            "pension",
            "stocks",
        ],
    ),
    (AccountType::Savings, &["savings", "saving", "save"]),
    (AccountType::Checking, &["checking", "chequing", "current"]),
    (AccountType::Cash, &["cash", "wallet", "purse"]),
];

impl AccountType {
    /// Every type, in the order the app lists them.
    pub const ALL: [AccountType; 7] = [
        AccountType::Checking,
        AccountType::Savings,
        AccountType::CreditCard,
        AccountType::Investment,
        AccountType::Loan,
        AccountType::Cash,
        AccountType::Other,
    ];

    /// The type spelled as stored; `None` for anything else.
    #[must_use]
    pub fn from_stored(text: &str) -> Option<AccountType> {
        AccountType::ALL
            .into_iter()
            .find(|kind| kind.as_str() == text)
    }

    /// As stored and as the API sends it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            AccountType::Checking => "checking",
            AccountType::Savings => "savings",
            AccountType::CreditCard => "credit_card",
            AccountType::Investment => "investment",
            AccountType::Loan => "loan",
            AccountType::Cash => "cash",
            AccountType::Other => "other",
        }
    }

    /// The importer's best guess from an account's name: "Rewards card" is a credit card. A name
    /// that says nothing is a checking account if it's on budget, and other if not *(agent)*.
    #[must_use]
    pub fn guess(name: &str, role: Role) -> AccountType {
        let lower = name.to_lowercase();
        let words: Vec<&str> = lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .collect();
        TYPE_WORDS
            .iter()
            .find(|(_, keys)| words.iter().any(|word| keys.contains(word)))
            .map_or(
                match role {
                    Role::Budgeted => AccountType::Checking,
                    Role::Tracking => AccountType::Other,
                },
                |(kind, _)| *kind,
            )
    }
}

/// Whether an account's activity counts toward budgets (budgeted) or only its balance over time
/// is shown (tracking).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Budgeted,
    Tracking,
}

impl Role {
    /// The role spelled as stored; `None` for anything else.
    #[must_use]
    pub fn from_stored(text: &str) -> Option<Role> {
        [Role::Budgeted, Role::Tracking]
            .into_iter()
            .find(|role| role.as_str() == text)
    }

    /// As stored and as the API sends it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Budgeted => "budgeted",
            Role::Tracking => "tracking",
        }
    }

    /// The role an Actual account gets: off budget is tracking, the rest budgeted.
    #[must_use]
    pub fn from_actual(off_budget: bool) -> Role {
        if off_budget {
            Role::Tracking
        } else {
            Role::Budgeted
        }
    }
}

/// An account's opening balance: derived, never a transaction, never income.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Opening {
    pub balance: Money,
    pub date: Date,
}

impl Opening {
    /// The opening balance the Actual import sets (docs/spec/m1-ledger-import.md#turning-actual-rows-into-transactions):
    /// the account's Actual balance minus its imported rows (`rows`, their total), dated the day
    /// before the earliest of them (`earliest`). With no rows, the whole balance, dated `today`
    /// *(agent)*. `None` if the sum doesn't fit or there's no day before.
    #[must_use]
    pub fn from_import(
        actual_balance: Money,
        rows: Money,
        earliest: Option<Date>,
        today: Date,
    ) -> Option<Opening> {
        Some(Opening {
            balance: actual_balance.checked_sub(rows)?,
            date: match earliest {
                Some(date) => date.previous_day()?,
                None => today,
            },
        })
    }
}

/// The app's own balance for an account: its opening balance plus its transactions. The balance
/// shown is the latest balance snapshot; where the two differ, that's drift (milestone 2). `None`
/// if the sum doesn't fit.
#[must_use]
pub fn app_balance(opening: Money, transactions: impl IntoIterator<Item = Money>) -> Option<Money> {
    opening.checked_add(Money::checked_sum(transactions)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guess(name: &str) -> AccountType {
        AccountType::guess(name, Role::Budgeted)
    }

    #[test]
    fn guesses_the_type_from_the_name() {
        assert_eq!(guess("Rewards card"), AccountType::CreditCard);
        assert_eq!(guess("VISA Signature"), AccountType::CreditCard);
        assert_eq!(guess("Cash back card"), AccountType::CreditCard);
        assert_eq!(guess("Car loan"), AccountType::Loan);
        assert_eq!(guess("Brokerage"), AccountType::Investment);
        assert_eq!(guess("Roth IRA"), AccountType::Investment);
        assert_eq!(guess("Rainy day savings"), AccountType::Savings);
        assert_eq!(guess("Everyday checking"), AccountType::Checking);
        assert_eq!(guess("Wallet"), AccountType::Cash);
    }

    #[test]
    fn only_whole_words_count() {
        // "Discard" holds "card", but not as a word.
        assert_eq!(guess("Discard pile"), AccountType::Checking);
        assert_eq!(guess("Joint-checking"), AccountType::Checking);
    }

    #[test]
    fn a_name_that_says_nothing_follows_the_role() {
        assert_eq!(guess("Household"), AccountType::Checking);
        assert_eq!(
            AccountType::guess("Household", Role::Tracking),
            AccountType::Other
        );
    }

    #[test]
    fn off_budget_is_tracking() {
        assert_eq!(Role::from_actual(true), Role::Tracking);
        assert_eq!(Role::from_actual(false), Role::Budgeted);
    }

    fn date(text: &str) -> Date {
        text.parse().unwrap()
    }

    #[test]
    fn the_imports_opening_balance_is_whats_left_before_the_first_row() {
        let opening = Opening::from_import(
            Money::from_minor(10_000),
            Money::from_minor(-2_500),
            Some(date("2025-01-01")),
            date("2026-10-30"),
        );
        assert_eq!(
            opening,
            Some(Opening {
                balance: Money::from_minor(12_500),
                date: date("2024-12-31"),
            })
        );
        // No rows: all of it, today.
        let opening = Opening::from_import(
            Money::from_minor(500),
            Money::ZERO,
            None,
            date("2026-10-30"),
        );
        assert_eq!(
            opening,
            Some(Opening {
                balance: Money::from_minor(500),
                date: date("2026-10-30"),
            })
        );
        let today = date("2026-10-30");
        assert_eq!(
            Opening::from_import(
                Money::from_minor(i64::MIN),
                Money::from_minor(1),
                None,
                today
            ),
            None
        );
        assert_eq!(
            Opening::from_import(Money::ZERO, Money::ZERO, Some(date("0001-01-01")), today),
            None
        );
    }

    #[test]
    fn the_app_balance_is_the_opening_plus_the_transactions() {
        let rows = [-2_500, 1_000, 0].map(Money::from_minor);
        assert_eq!(
            app_balance(Money::from_minor(12_500), rows),
            Some(Money::from_minor(11_000))
        );
        assert_eq!(
            app_balance(Money::from_minor(7), []),
            Some(Money::from_minor(7))
        );
        assert_eq!(
            app_balance(Money::from_minor(i64::MAX), [Money::from_minor(1)]),
            None
        );
    }

    #[test]
    fn spells_types_and_roles_as_stored() {
        let types = [
            (AccountType::Checking, "checking"),
            (AccountType::Savings, "savings"),
            (AccountType::CreditCard, "credit_card"),
            (AccountType::Investment, "investment"),
            (AccountType::Loan, "loan"),
            (AccountType::Cash, "cash"),
            (AccountType::Other, "other"),
        ];
        for (kind, text) in types {
            assert_eq!(kind.as_str(), text);
            assert_eq!(AccountType::from_stored(text), Some(kind));
        }
        assert_eq!(AccountType::from_stored("wallet"), None);
        assert_eq!(Role::Budgeted.as_str(), "budgeted");
        assert_eq!(Role::Tracking.as_str(), "tracking");
        assert_eq!(Role::from_stored("budgeted"), Some(Role::Budgeted));
        assert_eq!(Role::from_stored("tracking"), Some(Role::Tracking));
        assert_eq!(Role::from_stored("saved"), None);
    }
}
