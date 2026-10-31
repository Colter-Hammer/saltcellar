//! Transactions (docs/spec/m1-ledger-import.md#transactions): where a page of them ends, what a
//! search looks for, and the words for their status and where they came from.

use std::fmt;
use std::str::FromStr;

use crate::{Currency, Date, Money};

/// Where a page of transactions ended: the last row's date and id. Lists run newest first, by date
/// then id, so the next page is every row before this one in that order
/// (docs/spec/foundations.md#http-api-agent).
///
/// Written as `YYYY-MM-DD.id`, like `2026-01-05.42` *(agent)*: clients pass it back as they got it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub date: Date,
    pub id: i64,
}

impl fmt::Display for Cursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.date, self.id)
    }
}

/// A cursor that isn't one this API wrote.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a page cursor: {0:?}")]
pub struct BadCursor(String);

impl FromStr for Cursor {
    type Err = BadCursor;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || BadCursor(s.to_owned());
        let (date, id_text) = s.split_once('.').ok_or_else(bad)?;
        // Only the form written above: no sign, no leading zeros.
        let id = id_text
            .parse::<i64>()
            .ok()
            .filter(|id| *id > 0 && id.to_string() == id_text)
            .ok_or_else(bad)?;
        Ok(Cursor {
            date: date.parse().map_err(|_| bad())?,
            id,
        })
    }
}

/// Whether the bank has settled it. Pending rows count toward budgets straight away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pending,
    Posted,
}

impl Status {
    /// The status the database stored as `text`, if it's one.
    #[must_use]
    pub fn from_stored(text: &str) -> Option<Status> {
        match text {
            "pending" => Some(Status::Pending),
            "posted" => Some(Status::Posted),
            _ => None,
        }
    }
}

/// Where a transaction came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The bank, through the sync provider.
    Provider,
    /// A file: an import from Actual, a CSV or an OFX.
    Import,
    /// Entered by hand.
    Manual,
}

impl Source {
    /// The source as the database stores it.
    #[must_use]
    pub fn as_stored(self) -> &'static str {
        match self {
            Source::Provider => "provider",
            Source::Import => "import",
            Source::Manual => "manual",
        }
    }

    /// The source the database stored as `text`, if it's one.
    #[must_use]
    pub fn from_stored(text: &str) -> Option<Source> {
        [Source::Provider, Source::Import, Source::Manual]
            .into_iter()
            .find(|source| source.as_stored() == text)
    }
}

/// What the Transactions search looks for: the text, in payee, raw description and memo, and the
/// amount it reads as, if it reads as one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Search {
    /// Trimmed; never empty.
    pub text: String,
    /// In minor units, never negative: `12.50` matches −12.50 and 12.50 *(agent)*.
    pub amount_minor: Option<i64>,
}

impl Search {
    /// The search for `text` in a household that counts in `currency`; `None` when it's blank.
    #[must_use]
    pub fn new(text: &str, currency: Currency) -> Option<Search> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        Some(Search {
            text: text.to_owned(),
            amount_minor: amount_minor(text, currency.decimal_places()),
        })
    }
}

/// `text` as an amount in minor units, ignoring its sign, if it reads as one *(agent)*: digits,
/// then optionally a `.` or `,` and up to `places` more digits. `12` and `12.5` read as 1200 and
/// 1250 when `places` is 2. Group separators (`1,234.50`) don't read, as `,` may be the decimal one.
fn amount_minor(text: &str, places: u8) -> Option<i64> {
    let unsigned = text.strip_prefix(['-', '+', '−']).unwrap_or(text);
    let (whole, fraction) = match unsigned.find(['.', ',']) {
        Some(at) => (&unsigned[..at], &unsigned[at + 1..]),
        None => (unsigned, ""),
    };
    let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    if whole.is_empty() || !digits(whole) || !digits(fraction) {
        return None;
    }
    if fraction.len() > usize::from(places) {
        return None;
    }
    let scale = 10_i64.checked_pow(u32::from(places))?;
    let fraction_scale =
        10_i64.checked_pow(u32::from(places) - u32::try_from(fraction.len()).ok()?)?;
    let fraction_minor = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<i64>().ok()?.checked_mul(fraction_scale)?
    };
    whole
        .parse::<i64>()
        .ok()?
        .checked_mul(scale)?
        .checked_add(fraction_minor)
}

/// The Transactions category filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CategoryFilter {
    /// No category, not split, and not a transfer leg.
    Uncategorized,
    /// In this category, or split with a line in it *(agent)*.
    Category(i64),
}

/// Whether a split's lines add up to the transaction's amount: the invariant every split keeps.
/// A split with no lines isn't one, and amounts too big to add don't add up.
#[must_use]
pub fn lines_add_up(total: Money, lines: impl IntoIterator<Item = Money>) -> bool {
    let mut lines = lines.into_iter().peekable();
    lines.peek().is_some() && Money::checked_sum(lines) == Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_lines_must_add_up_to_the_amount() {
        let m = Money::from_minor;
        assert!(lines_add_up(m(-3_005), [m(-2_000), m(-1_005)]));
        assert!(lines_add_up(m(-3_005), [m(-2_000), m(-1_005), m(0)]));
        assert!(lines_add_up(m(500), [m(900), m(-400)]));
        assert!(!lines_add_up(m(-3_005), [m(-2_000), m(-1_000)]));
        assert!(!lines_add_up(m(-3_005), [m(-3_005), m(-3_005)]));
        assert!(!lines_add_up(m(0), []));
        assert!(!lines_add_up(m(-1), [m(i64::MAX), m(i64::MAX)]));
    }

    #[test]
    fn a_cursor_reads_back_what_it_writes() {
        let cursor = Cursor {
            date: "2026-01-05".parse().unwrap(),
            id: 42,
        };
        assert_eq!(cursor.to_string(), "2026-01-05.42");
        assert_eq!("2026-01-05.42".parse(), Ok(cursor));
    }

    #[test]
    fn a_cursor_it_didnt_write_is_refused() {
        for bad in [
            "",
            "2026-01-05",
            "2026-01-05.",
            "2026-01-05.x",
            "2026-01-05.0",
            "2026-01-05.-3",
            "2026-01-05.+3",
            "2026-01-05.03",
            "2026-02-30.3",
            "yesterday.3",
        ] {
            assert_eq!(
                bad.parse::<Cursor>(),
                Err(BadCursor(bad.to_owned())),
                "{bad}"
            );
        }
    }

    #[test]
    fn statuses_read_as_stored() {
        assert_eq!(Status::from_stored("pending"), Some(Status::Pending));
        assert_eq!(Status::from_stored("posted"), Some(Status::Posted));
        assert_eq!(Status::from_stored("cleared"), None);
    }

    #[test]
    fn sources_read_back_as_stored() {
        for source in [Source::Provider, Source::Import, Source::Manual] {
            assert_eq!(Source::from_stored(source.as_stored()), Some(source));
        }
        assert_eq!(Source::from_stored("bank"), None);
    }

    fn search(text: &str, currency: &str) -> Option<Search> {
        Search::new(text, currency.parse().unwrap())
    }

    #[test]
    fn a_blank_search_is_none() {
        assert_eq!(search("", "USD"), None);
        assert_eq!(search("   ", "USD"), None);
    }

    #[test]
    fn a_search_reads_whole_and_decimal_amounts_in_the_currency() {
        let amount = |text: &str, currency: &str| search(text, currency).unwrap().amount_minor;
        assert_eq!(amount("12.50", "USD"), Some(1250));
        assert_eq!(amount(" 12.50 ", "USD"), Some(1250));
        assert_eq!(amount("-12.50", "USD"), Some(1250));
        assert_eq!(amount("+12.5", "USD"), Some(1250));
        assert_eq!(amount("12,5", "EUR"), Some(1250));
        assert_eq!(amount("12", "USD"), Some(1200));
        assert_eq!(amount("12.", "USD"), Some(1200));
        assert_eq!(amount("0.07", "USD"), Some(7));
        assert_eq!(amount("1500", "JPY"), Some(1500));
        assert_eq!(amount("1.234", "KWD"), Some(1234));
        // Not amounts: only their text is searched.
        assert_eq!(amount("12.505", "USD"), None);
        assert_eq!(amount("12.5", "JPY"), None);
        assert_eq!(amount("1,234.50", "USD"), None);
        assert_eq!(amount(".50", "USD"), None);
        assert_eq!(amount("-", "USD"), None);
        assert_eq!(amount("12a", "USD"), None);
        assert_eq!(amount("corner", "USD"), None);
        assert_eq!(amount("99999999999999999999", "USD"), None);
        assert_eq!(amount("99999999999999999", "USD"), None);
        assert_eq!(search(" Corner ", "USD").unwrap().text, "Corner");
    }
}
