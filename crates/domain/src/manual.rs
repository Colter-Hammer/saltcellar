//! Manual entry (docs/spec/m2-bank-data.md#manual-entry): a transaction typed in by hand, checked
//! before it's written. It lands in a category, or is one leg of a transfer to another account;
//! never neither, so a manual row never waits in Review.

use std::collections::BTreeMap;

use crate::{Date, Money};

/// Where a manual row's amount goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lands {
    /// In this category.
    Category(i64),
    /// Moved to (or, money in, from) this other account: a one-leg transfer, no category.
    TransferWith(i64),
}

/// A manual row as typed, before it's checked.
#[derive(Debug, Clone, Default)]
pub struct Typed<'a> {
    pub account_id: i64,
    /// `YYYY-MM-DD`.
    pub date: &'a str,
    /// Signed: negative is money out.
    pub amount_minor: i64,
    pub payee: Option<&'a str>,
    pub category_id: Option<i64>,
    pub transfer_account_id: Option<i64>,
    pub memo: Option<&'a str>,
    pub tags: &'a [String],
}

/// A checked manual row, ready to be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub account_id: i64,
    pub date: Date,
    pub amount: Money,
    /// Trimmed; `None` when blank.
    pub payee: Option<String>,
    pub lands: Lands,
    /// Trimmed; `None` when blank.
    pub memo: Option<String>,
    /// Without `#`, once each, in the order typed.
    pub tags: Vec<String>,
}

impl Entry {
    /// What stands in for the bank's text on a manual row, which has none: the payee, or empty
    /// *(agent)*. The dedupe key is made from it.
    #[must_use]
    pub fn raw_description(&self) -> &str {
        self.payee.as_deref().unwrap_or("")
    }
}

/// The longest payee *(agent)*.
pub const PAYEE_MAX_CHARS: usize = 100;
/// The longest memo *(agent)*.
pub const MEMO_MAX_CHARS: usize = 1000;
/// The longest tag *(agent)*.
pub const TAG_MAX_CHARS: usize = 40;

/// Each field's problem, by the field's name in the API.
pub type FieldErrors = BTreeMap<&'static str, String>;

/// `typed` checked: every problem named at once, by field.
///
/// # Errors
/// The fields that can't be written as typed, each with what to do about it.
pub fn check(typed: &Typed<'_>) -> Result<Entry, FieldErrors> {
    let mut errors = FieldErrors::new();
    let date = typed.date.parse::<Date>().ok();
    if date.is_none() {
        errors.insert("date", "Enter a date.".to_owned());
    }
    if typed.amount_minor == 0 {
        errors.insert("amount_minor", "Enter an amount other than zero.".to_owned());
    }
    let payee = trimmed(typed.payee);
    if payee
        .as_ref()
        .is_some_and(|payee| payee.chars().count() > PAYEE_MAX_CHARS)
    {
        errors.insert(
            "payee",
            format!("Use at most {PAYEE_MAX_CHARS} characters."),
        );
    }
    let memo = trimmed(typed.memo);
    if memo
        .as_ref()
        .is_some_and(|memo| memo.chars().count() > MEMO_MAX_CHARS)
    {
        errors.insert("memo", format!("Use at most {MEMO_MAX_CHARS} characters."));
    }
    let lands = match (typed.category_id, typed.transfer_account_id) {
        (Some(_), Some(_)) => {
            errors.insert(
                "category_id",
                "Choose a category or a transfer, not both.".to_owned(),
            );
            None
        }
        (Some(category), None) => Some(Lands::Category(category)),
        (None, Some(other)) if other == typed.account_id => {
            errors.insert(
                "transfer_account_id",
                "Choose an account other than this one.".to_owned(),
            );
            None
        }
        (None, Some(other)) => Some(Lands::TransferWith(other)),
        (None, None) => {
            errors.insert("category_id", "Choose a category.".to_owned());
            None
        }
    };
    let tags = match tag_names(typed.tags) {
        Ok(tags) => tags,
        Err(message) => {
            errors.insert("tags", message);
            Vec::new()
        }
    };
    match (date, lands) {
        (Some(date), Some(lands)) if errors.is_empty() => Ok(Entry {
            account_id: typed.account_id,
            date,
            amount: Money::from_minor(typed.amount_minor),
            payee,
            lands,
            memo,
            tags,
        }),
        _ => Err(errors),
    }
}

fn trimmed(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// The tags as typed, each trimmed and without a leading `#`, blanks dropped, once each *(agent)*.
/// A tag is letters, digits, `-` and `_`, as a `#word` in Actual's notes is.
fn tag_names(typed: &[String]) -> Result<Vec<String>, String> {
    let mut tags: Vec<String> = Vec::new();
    for tag in typed {
        let tag = tag.trim();
        let tag = tag.strip_prefix('#').unwrap_or(tag);
        if tag.is_empty() {
            continue;
        }
        if !tag
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(format!(
                "Use letters, digits, - and _ in a tag; \"{tag}\" has something else."
            ));
        }
        if tag.chars().count() > TAG_MAX_CHARS {
            return Err(format!("Use at most {TAG_MAX_CHARS} characters in a tag."));
        }
        if !tags.iter().any(|seen| seen == tag) {
            tags.push(tag.to_owned());
        }
    }
    Ok(tags)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed<'a>(tags: &'a [String]) -> Typed<'a> {
        Typed {
            account_id: 1,
            date: "2026-03-04",
            amount_minor: -1250,
            payee: Some("  Corner Market "),
            category_id: Some(7),
            transfer_account_id: None,
            memo: Some(" "),
            tags,
        }
    }

    #[test]
    fn a_full_entry_checks_out_trimmed() {
        let tags = ["#trip".to_owned(), " gift ".to_owned(), "trip".to_owned(), String::new()];
        let entry = check(&typed(&tags)).unwrap();
        assert_eq!(
            entry,
            Entry {
                account_id: 1,
                date: "2026-03-04".parse().unwrap(),
                amount: Money::from_minor(-1250),
                payee: Some("Corner Market".to_owned()),
                lands: Lands::Category(7),
                memo: None,
                tags: vec!["trip".to_owned(), "gift".to_owned()],
            }
        );
        assert_eq!(entry.raw_description(), "Corner Market");
    }

    #[test]
    fn a_transfer_needs_no_category_but_another_account() {
        let entry = check(&Typed {
            category_id: None,
            transfer_account_id: Some(2),
            payee: None,
            ..typed(&[])
        })
        .unwrap();
        assert_eq!(entry.lands, Lands::TransferWith(2));
        assert_eq!(entry.raw_description(), "");

        let errors = check(&Typed {
            category_id: None,
            transfer_account_id: Some(1),
            ..typed(&[])
        })
        .unwrap_err();
        assert_eq!(
            errors.keys().copied().collect::<Vec<_>>(),
            ["transfer_account_id"]
        );
    }

    #[test]
    fn a_category_is_required_unless_its_a_transfer() {
        let neither = check(&Typed {
            category_id: None,
            ..typed(&[])
        })
        .unwrap_err();
        assert_eq!(neither["category_id"], "Choose a category.");
        let both = check(&Typed {
            transfer_account_id: Some(2),
            ..typed(&[])
        })
        .unwrap_err();
        assert!(both.contains_key("category_id"));
    }

    #[test]
    fn every_problem_is_named_at_once() {
        let long_payee = "p".repeat(PAYEE_MAX_CHARS + 1);
        let long_memo = "m".repeat(MEMO_MAX_CHARS + 1);
        let tags = ["no spaces".to_owned()];
        let errors = check(&Typed {
            date: "2026-02-30",
            amount_minor: 0,
            payee: Some(&long_payee),
            memo: Some(&long_memo),
            category_id: None,
            ..typed(&tags)
        })
        .unwrap_err();
        assert_eq!(
            errors.keys().copied().collect::<Vec<_>>(),
            ["amount_minor", "category_id", "date", "memo", "payee", "tags"]
        );
    }

    #[test]
    fn tags_are_words() {
        let long = ["t".repeat(TAG_MAX_CHARS + 1)];
        assert!(check(&typed(&long)).unwrap_err().contains_key("tags"));
        let fine = ["café_2026-trip".to_owned()];
        assert_eq!(check(&typed(&fine)).unwrap().tags, fine);
    }
}
