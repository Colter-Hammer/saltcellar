//! Transactions (docs/spec/m1-ledger-import.md#transactions): where a page of them ends, and the
//! words for their status and where they came from.

use std::fmt;
use std::str::FromStr;

use crate::Date;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
