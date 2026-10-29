//! Category groups and categories: their names, the starter set, and the rules for rearranging
//! them (docs/spec/m1-ledger-import.md#categories).

use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

/// Whether a group's categories are spending or money coming in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GroupKind {
    Expense,
    Income,
}

impl GroupKind {
    /// As stored and as the API sends it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            GroupKind::Expense => "expense",
            GroupKind::Income => "income",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} isn't a group kind")]
pub struct UnknownGroupKind(pub String);

impl FromStr for GroupKind {
    type Err = UnknownGroupKind;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "expense" => Ok(GroupKind::Expense),
            "income" => Ok(GroupKind::Income),
            _ => Err(UnknownGroupKind(text.to_owned())),
        }
    }
}

/// A group's or a category's name: any text, trimmed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Name(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    #[error("Enter a name.")]
    Empty,
    #[error("Use at most {} characters.", Name::MAX_CHARS)]
    TooLong,
}

impl Name {
    /// Room for any sensible name *(agent)*.
    pub const MAX_CHARS: usize = 64;

    /// # Errors
    /// If it's empty once trimmed, or too long.
    pub fn new(typed: &str) -> Result<Self, NameError> {
        let name = typed.trim();
        if name.is_empty() {
            return Err(NameError::Empty);
        }
        if name.chars().count() > Self::MAX_CHARS {
            return Err(NameError::TooLong);
        }
        Ok(Name(name.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A group of the starter set, with its categories in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StarterGroup {
    pub name: &'static str,
    pub kind: GroupKind,
    pub categories: &'static [&'static str],
}

const fn expense(name: &'static str, categories: &'static [&'static str]) -> StarterGroup {
    StarterGroup {
        name,
        kind: GroupKind::Expense,
        categories,
    }
}

/// The starter categories, in order: generic, with deliberately no catch-all.
pub const STARTER_GROUPS: &[StarterGroup] = &[
    expense(
        "Housing",
        &[
            "Mortgage & Rent",
            "Utilities",
            "Internet & Phone",
            "Home Maintenance",
        ],
    ),
    expense("Food", &["Groceries", "Restaurants", "Fast Food & Coffee"]),
    expense(
        "Transportation",
        &["Gas", "Car Maintenance", "Car Purchase"],
    ),
    expense(
        "Health & Fitness",
        &["Medical", "Dental & Vision", "Fitness"],
    ),
    expense("Insurance", &["Insurance"]),
    expense("Kids", &["Activities", "School & Childcare"]),
    expense("Shopping", &["Household", "Clothing", "Personal Care"]),
    expense("Fun", &["Entertainment", "Travel", "Subscriptions"]),
    expense("Giving", &["Tithing & Donations", "Gifts"]),
    expense("Pets", &["Pet Care"]),
    expense("Financial", &["Bank Fees & Interest", "Taxes"]),
    StarterGroup {
        name: "Income",
        kind: GroupKind::Income,
        categories: &["Paycheck", "Interest", "Tax Refunds", "Other Income"],
    },
];

/// The payees the starter set seeds as always review: merchants whose charges can be anything.
pub const ALWAYS_REVIEW_PAYEES: &[&str] = &[
    "Amazon",
    "Target",
    "Walmart",
    "Costco",
    "Sam's Club",
    "Venmo",
    "Cash Withdrawal",
    "Check",
    "eBay",
];

/// One group's place in the order, with its categories' ids in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupOrder {
    pub group_id: i64,
    pub category_ids: Vec<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OrderError {
    #[error("group {0} is listed twice")]
    GroupTwice(i64),
    #[error("category {0} is listed twice")]
    CategoryTwice(i64),
    #[error("the order doesn't list the same groups and categories there are")]
    Different,
}

/// Checks that `proposed` rearranges `current`: every group listed once, every category listed
/// once, so in exactly one group, and nothing added or left out. Hidden ones are listed too.
///
/// # Errors
/// If a group or a category is listed twice, or the two don't hold the same ones.
pub fn check_order(current: &[GroupOrder], proposed: &[GroupOrder]) -> Result<(), OrderError> {
    let mut groups = HashSet::new();
    let mut categories = HashSet::new();
    for group in proposed {
        if !groups.insert(group.group_id) {
            return Err(OrderError::GroupTwice(group.group_id));
        }
        for &id in &group.category_ids {
            if !categories.insert(id) {
                return Err(OrderError::CategoryTwice(id));
            }
        }
    }
    let current_groups: HashSet<i64> = current.iter().map(|g| g.group_id).collect();
    let current_categories: HashSet<i64> = current
        .iter()
        .flat_map(|g| g.category_ids.iter().copied())
        .collect();
    if groups != current_groups || categories != current_categories {
        return Err(OrderError::Different);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_starter_group_but_income_is_spending() {
        for group in &STARTER_GROUPS[..STARTER_GROUPS.len() - 1] {
            assert_eq!(
                *group,
                expense(group.name, group.categories),
                "{}",
                group.name
            );
        }
        assert_eq!(
            STARTER_GROUPS.last().map(|g| g.kind),
            Some(GroupKind::Income)
        );
    }

    #[test]
    fn kinds_read_back_what_they_store() {
        for kind in [GroupKind::Expense, GroupKind::Income] {
            assert_eq!(kind.as_str().parse::<GroupKind>(), Ok(kind));
        }
        assert_eq!(
            "Income".parse::<GroupKind>(),
            Err(UnknownGroupKind("Income".to_owned()))
        );
        assert_eq!(
            UnknownGroupKind("x".to_owned()).to_string(),
            "\"x\" isn't a group kind"
        );
    }

    #[test]
    fn names_are_trimmed_and_need_something() {
        let name = Name::new("  Fast Food & Coffee ").unwrap();
        assert_eq!(name.as_str(), "Fast Food & Coffee");
        assert_eq!(name.to_string(), "Fast Food & Coffee");
        assert_eq!(Name::new(" \t "), Err(NameError::Empty));
        assert!(Name::new(&"é".repeat(64)).is_ok());
        assert_eq!(Name::new(&"é".repeat(65)), Err(NameError::TooLong));
        assert_eq!(NameError::Empty.to_string(), "Enter a name.");
        assert_eq!(NameError::TooLong.to_string(), "Use at most 64 characters.");
    }

    #[test]
    fn the_starter_set_is_the_specs_twelve_groups_with_income_last() {
        let names: Vec<&str> = STARTER_GROUPS.iter().map(|g| g.name).collect();
        assert_eq!(
            names,
            [
                "Housing",
                "Food",
                "Transportation",
                "Health & Fitness",
                "Insurance",
                "Kids",
                "Shopping",
                "Fun",
                "Giving",
                "Pets",
                "Financial",
                "Income",
            ]
        );
        let income: Vec<&str> = STARTER_GROUPS
            .iter()
            .filter(|g| g.kind == GroupKind::Income)
            .map(|g| g.name)
            .collect();
        assert_eq!(income, ["Income"]);
        let count: usize = STARTER_GROUPS.iter().map(|g| g.categories.len()).sum();
        assert_eq!(count, 31);
        assert_eq!(ALWAYS_REVIEW_PAYEES.len(), 9);
    }

    #[test]
    fn every_starter_name_is_a_valid_name() {
        for group in STARTER_GROUPS {
            assert_eq!(Name::new(group.name).unwrap().as_str(), group.name);
            for category in group.categories {
                assert_eq!(Name::new(category).unwrap().as_str(), *category);
            }
        }
    }

    fn order(groups: &[(i64, &[i64])]) -> Vec<GroupOrder> {
        groups
            .iter()
            .map(|(group_id, ids)| GroupOrder {
                group_id: *group_id,
                category_ids: ids.to_vec(),
            })
            .collect()
    }

    #[test]
    fn any_rearrangement_of_the_same_groups_and_categories_is_fine() {
        let current = order(&[(1, &[10, 11]), (2, &[20]), (3, &[])]);
        assert_eq!(check_order(&current, &current), Ok(()));
        let moved = order(&[(3, &[11]), (1, &[20, 10]), (2, &[])]);
        assert_eq!(check_order(&current, &moved), Ok(()));
    }

    #[test]
    fn a_category_cant_be_in_two_groups() {
        let current = order(&[(1, &[10, 11]), (2, &[20])]);
        let twice = order(&[(1, &[10, 11]), (2, &[20, 10])]);
        assert_eq!(
            check_order(&current, &twice),
            Err(OrderError::CategoryTwice(10))
        );
        let same_group = order(&[(1, &[10, 11, 11]), (2, &[20])]);
        assert_eq!(
            check_order(&current, &same_group),
            Err(OrderError::CategoryTwice(11))
        );
    }

    #[test]
    fn a_group_cant_be_listed_twice() {
        let current = order(&[(1, &[10]), (2, &[])]);
        let twice = order(&[(1, &[10]), (2, &[]), (1, &[])]);
        assert_eq!(
            check_order(&current, &twice),
            Err(OrderError::GroupTwice(1))
        );
        assert_eq!(
            OrderError::GroupTwice(1).to_string(),
            "group 1 is listed twice"
        );
    }

    #[test]
    fn nothing_can_be_left_out_or_made_up() {
        let current = order(&[(1, &[10, 11]), (2, &[20])]);
        for proposed in [
            order(&[(1, &[10, 11])]),
            order(&[(1, &[10]), (2, &[20])]),
            order(&[(1, &[10, 11]), (2, &[20, 99])]),
            order(&[(1, &[10, 11]), (2, &[20]), (9, &[])]),
        ] {
            assert_eq!(
                check_order(&current, &proposed),
                Err(OrderError::Different),
                "{proposed:?}"
            );
        }
    }
}
