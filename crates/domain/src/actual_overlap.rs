//! Import from Actual, overlap-aware (docs/spec/m2-bank-data.md#import-from-actual-becomes-overlap-aware):
//! what becomes of each Actual row going into an account that has rows already, synced or from an
//! earlier import.
//!
//! In order, for each row:
//! 1. An earlier Actual import, not undone, brought this very Actual row into the account: it's
//!    [`Overlap::Seen`], and nothing changes *(agent)*.
//! 2. A stored row has its bank id (`external_id`): [`Overlap::SameId`], the same row.
//! 3. Inside the period the connection has synced (from the oldest synced row on), the file rule
//!    ([`crate::csv_import::plan`]: exact amount, ±3 days, nearest date then a similar
//!    description, repeats counted): [`Overlap::Matched`], or a tie, [`Overlap::Check`].
//! 4. Anything else is [`Overlap::New`]: older than the synced period, or nothing there is it.
//!
//! A stored row is taken at most once. Pure: the caller loads the stored rows and applies what
//! comes back.

use std::collections::{BTreeSet, HashMap};

use crate::csv_import::{self, FileRow, Outcome, Stored};
use crate::import::Row;
use crate::{Date, Money};

/// A row the account already has, as overlap matching sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRow {
    pub id: i64,
    /// The bank's id for it, if it has one.
    pub external_id: Option<String>,
    /// The Actual transaction id it came in under, from an earlier Actual import not undone.
    pub actual_id: Option<String>,
    pub date: Date,
    pub amount: Money,
    pub raw_description: String,
}

/// What becomes of one Actual row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlap {
    /// An earlier Actual import brought it: already there, left as it is.
    Seen(i64),
    /// The stored row with the same bank id: it keeps the bank's fields and takes Actual's.
    SameId(i64),
    /// The stored row the file rule finds: it's the same row, as [`Overlap::SameId`] is.
    Matched(i64),
    /// It could be this stored row, or another as good: the household decides. Unless they say
    /// it's new, it's taken as this row *(agent)*.
    Check(i64),
    /// Inserted.
    New,
}

/// What becomes of each of an account's Actual `rows`, in their order, given the account's
/// `stored` rows and the date of its oldest synced row (`None`: never synced, so no file rule).
#[must_use]
pub fn overlap(rows: &[Row], stored: &[StoredRow], synced_from: Option<Date>) -> Vec<Overlap> {
    let by_actual: HashMap<&str, i64> = stored
        .iter()
        .filter_map(|s| Some((s.actual_id.as_deref()?, s.id)))
        .collect();
    let by_bank_id: HashMap<&str, i64> = stored
        .iter()
        .filter_map(|s| Some((s.external_id.as_deref()?, s.id)))
        .collect();
    let mut taken: BTreeSet<i64> = BTreeSet::new();
    let mut outcomes: Vec<Option<Overlap>> = rows
        .iter()
        .map(|row| {
            let seen = by_actual
                .get(row.import_ref.as_str())
                .filter(|id| !taken.contains(*id))
                .map(|id| Overlap::Seen(*id));
            let same = || {
                let id = by_bank_id.get(row.external_id.as_deref()?)?;
                (!taken.contains(id)).then_some(Overlap::SameId(*id))
            };
            let outcome = seen.or_else(same)?;
            if let Overlap::Seen(id) | Overlap::SameId(id) = outcome {
                taken.insert(id);
            }
            Some(outcome)
        })
        .collect();

    // The rest inside the synced period go through the file rule, against the rows not taken.
    let in_period: Vec<FileRow> = rows
        .iter()
        .enumerate()
        .filter(|(at, row)| {
            outcomes[*at].is_none() && synced_from.is_some_and(|from| row.date >= from)
        })
        .map(|(at, row)| FileRow {
            row: at,
            date: row.date,
            post_date: None,
            description: row.raw_description.clone(),
            amount: row.amount,
            category: None,
            memo: None,
            import_ref: row.import_ref.clone(),
        })
        .collect();
    let left: Vec<Stored> = stored
        .iter()
        .filter(|s| !taken.contains(&s.id))
        .map(|s| Stored {
            id: s.id,
            date: s.date,
            amount: s.amount,
            raw_description: s.raw_description.clone(),
        })
        .collect();
    for entry in csv_import::plan(in_period, &HashMap::new(), &left) {
        outcomes[entry.row.row] = Some(match entry.outcome {
            Outcome::Matched(id) => Overlap::Matched(id),
            Outcome::Check(id) => Overlap::Check(id),
            Outcome::Seen | Outcome::New => Overlap::New,
        });
    }
    outcomes
        .into_iter()
        .map(|outcome| outcome.unwrap_or(Overlap::New))
        .collect()
}

#[cfg(test)]
mod tests;
