//! Matching a synced row to a stored one (docs/spec/m2-bank-data.md#matching-a-synced-row),
//! steps 1 to 5: the same id, a pending row posting under a new id, an id the bank re-issued, an
//! imported row with no bank id taken over, and a manual row taken over. Step 0 (an id the
//! household deleted) is the caller's: those rows never reach here. Step 6 (insert) is what's
//! left.
//!
//! Pure: the caller loads the account's stored rows near the incoming dates and applies what
//! comes back.

use std::collections::BTreeSet;

use crate::feed::SyncedRow;
use crate::import::normalize;
use crate::{Date, Money};

/// A row already stored on the account, as matching sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRow {
    pub id: i64,
    /// The provider's id, if it has one.
    pub external_id: Option<String>,
    pub date: Date,
    pub amount: Money,
    pub pending: bool,
    pub raw_description: String,
    /// It came from a file or the Actual import (`source = 'import'`).
    pub imported: bool,
    /// The household typed it in (`source = 'manual'`); with no bank id, sync may adopt it.
    pub manual: bool,
}

/// What to do with one incoming row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    /// Step 1: the stored row with the same id.
    SameId(i64),
    /// Step 2: the stored pending row it posts; it takes that row over, the household's fields
    /// and all.
    Posts(i64),
    /// Step 3: the stored row the bank re-issued under this new id.
    Reissued(i64),
    /// Step 4 or 5: the imported or manual row with no bank id it takes over.
    Adopts(i64),
    /// Step 5: insert it, flagged `possible_manual_match` with this manual row, whose amount it
    /// exceeds by up to a quarter. Only the household merges them.
    PossibleManualMatch(i64),
    /// Insert it, flagged `possible_duplicate` of this stored row.
    PossibleDuplicate(i64),
    /// Insert it.
    New,
}

/// Days after the pending date a posted row may carry and still post it.
const POSTING_DAYS: i32 = 7;
/// Days either side a re-issued id's row may move.
const REISSUE_DAYS: i32 = 1;
/// Days either side an imported row's date may sit from the bank's.
const ADOPT_DAYS: i32 = 3;
/// Days before a manual row's date the bank's may be: a card payment entered a little late.
const MANUAL_DAYS_BEFORE: i32 = 3;
/// Days after a manual row's date the bank's may be: a check cashed later.
const MANUAL_DAYS_AFTER: i32 = 14;

/// The earliest date a stored row can be and still match an incoming row dated `date`: the
/// caller loads stored rows from here on.
#[must_use]
pub fn earliest_candidate(date: Date) -> Option<Date> {
    date.plus_days(-POSTING_DAYS.max(ADOPT_DAYS).max(MANUAL_DAYS_AFTER))
}

/// "Similar description" *(agent)*: after the shared normalization, one text starts with the
/// other, or they share their first word and their first 8 characters. Pending text is often
/// truncated. A text that normalizes to nothing is similar to none.
#[must_use]
pub fn similar_description(a: &str, b: &str) -> bool {
    let (a, b) = (normalize(a), normalize(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a.starts_with(&b) || b.starts_with(&a) {
        return true;
    }
    let first_word = |text: &str| text.split(' ').next().map(str::to_owned);
    let first_8 = |text: &str| -> Option<String> {
        (text.chars().count() >= 8).then(|| text.chars().take(8).collect())
    };
    first_word(&a) == first_word(&b) && first_8(&a).is_some() && first_8(&a) == first_8(&b)
}

/// A posted `amount` that could be pending `pending` settled: the same sign, equal or up to 25%
/// larger in size (a tip).
fn posts_for(amount: Money, pending: Money) -> bool {
    let (posted, held) = (amount.minor(), pending.minor());
    if posted.signum() != held.signum() {
        return false;
    }
    let (posted, held) = (i128::from(posted).abs(), i128::from(held).abs());
    posted >= held && posted * 4 <= held * 5
}

/// Step 5 among `candidates`, the manual rows with no bank id in `row`'s window: exactly one of
/// the same amount is adopted, several are a possible duplicate (of the oldest). With none,
/// exactly one the bank amount exceeds by up to a quarter is a possible manual match *(agent:
/// candidates of another amount, or more than a quarter smaller, don't count against it)*.
fn manual_adoption(row: &SyncedRow, mut candidates: Vec<&StoredRow>) -> Match {
    candidates.sort_by_key(|s| (s.date, s.id));
    let exact: Vec<&&StoredRow> = candidates
        .iter()
        .filter(|s| s.amount == row.amount)
        .collect();
    match exact.as_slice() {
        [only] => return Match::Adopts(only.id),
        [first, ..] => return Match::PossibleDuplicate(first.id),
        [] => {}
    }
    let near: Vec<&&StoredRow> = candidates
        .iter()
        .filter(|s| posts_for(row.amount, s.amount))
        .collect();
    match near.as_slice() {
        [only] => Match::PossibleManualMatch(only.id),
        _ => Match::New,
    }
}

/// Step 4 among `candidates`, the imported rows `row` could be: the nearest date wins, then a
/// similar description; copies of the same row are taken one each, and a real tie is flagged
/// *(agent)*.
fn adoption(row: &SyncedRow, mut candidates: Vec<&StoredRow>) -> Match {
    let rank = |s: &StoredRow| {
        (
            s.date.days_until(row.date).abs(),
            !similar_description(&s.raw_description, &row.raw_description),
        )
    };
    candidates.sort_by_key(|s| (rank(s), s.id));
    let Some(best) = candidates.first() else {
        return Match::New;
    };
    // As good a match as the best, but not a copy of it.
    let rival = |s: &StoredRow| {
        rank(s) == rank(best)
            && (s.date != best.date
                || normalize(&s.raw_description) != normalize(&best.raw_description))
    };
    if candidates.iter().any(|s| rival(s)) {
        Match::PossibleDuplicate(best.id)
    } else {
        Match::Adopts(best.id)
    }
}

/// Steps 4 and 5 for `row` among `stored`, the rows in `taken` aside: an imported row with no
/// bank id, else a manual one, by its current (edited) values.
fn adopt(row: &SyncedRow, stored: &[StoredRow], taken: &BTreeSet<i64>) -> Match {
    let free = |s: &&StoredRow| !taken.contains(&s.id) && s.external_id.is_none();
    let imported: Vec<&StoredRow> = stored
        .iter()
        .filter(free)
        .filter(|s| s.imported && s.amount == row.amount)
        .filter(|s| s.date.days_until(row.date).abs() <= ADOPT_DAYS)
        .collect();
    let found = adoption(row, imported);
    if found != Match::New {
        return found;
    }
    let manual: Vec<&StoredRow> = stored
        .iter()
        .filter(free)
        .filter(|s| s.manual)
        .filter(|s| {
            (-MANUAL_DAYS_BEFORE..=MANUAL_DAYS_AFTER).contains(&s.date.days_until(row.date))
        })
        .collect();
    manual_adoption(row, manual)
}

/// What each of `incoming` (one account's rows from this run's response) matches among
/// `stored` (that account's rows), in the same order as `incoming`. A stored row is matched at
/// most once. Incoming rows are matched oldest first, so several pending rows posting at once
/// pair oldest with oldest.
#[must_use]
pub fn match_rows(incoming: &[SyncedRow], stored: &[StoredRow]) -> Vec<Match> {
    let in_response: BTreeSet<&str> = incoming.iter().map(|r| r.external_id.as_str()).collect();
    // Its own id wasn't in this response.
    let absent = |s: &StoredRow| {
        s.external_id
            .as_deref()
            .is_some_and(|id| !in_response.contains(id))
    };
    let mut taken: BTreeSet<i64> = BTreeSet::new();
    let mut matches = vec![Match::New; incoming.len()];

    for (i, row) in incoming.iter().enumerate() {
        let same = stored
            .iter()
            .find(|s| s.external_id.as_deref() == Some(row.external_id.as_str()));
        if let Some(same) = same {
            taken.insert(same.id);
            matches[i] = Match::SameId(same.id);
        }
    }

    let mut order: Vec<usize> = (0..incoming.len())
        .filter(|&i| matches[i] == Match::New)
        .collect();
    order.sort_by(|&a, &b| {
        (incoming[a].date, &incoming[a].external_id)
            .cmp(&(incoming[b].date, &incoming[b].external_id))
    });
    // Oldest first among candidates, the lower id breaking ties.
    let oldest = |c: &&StoredRow| (c.date, c.id);

    for i in order {
        let row = &incoming[i];
        let free = |s: &&StoredRow| !taken.contains(&s.id);

        // Step 2: pending → posted under a new id.
        if !row.pending {
            let mut candidates: Vec<&StoredRow> = stored
                .iter()
                .filter(free)
                .filter(|s| s.pending && absent(s))
                .filter(|s| (0..=POSTING_DAYS).contains(&s.date.days_until(row.date)))
                .filter(|s| posts_for(row.amount, s.amount))
                .filter(|s| similar_description(&s.raw_description, &row.raw_description))
                .collect();
            candidates.sort_by_key(oldest);
            let paired = match candidates.as_slice() {
                [] => None,
                [only] => Some(Some(only.id)),
                several => Some(
                    several
                        .iter()
                        .find(|s| s.amount == row.amount)
                        .map(|s| s.id),
                ),
            };
            match paired {
                Some(Some(id)) => {
                    taken.insert(id);
                    matches[i] = Match::Posts(id);
                    continue;
                }
                // Several, none exact: insert, and the pending rows retire on their own.
                Some(None) => continue,
                None => {}
            }
        }

        // Step 3: an id re-issued.
        let mut candidates: Vec<&StoredRow> = stored
            .iter()
            .filter(free)
            .filter(|s| s.pending == row.pending && absent(s) && s.amount == row.amount)
            .filter(|s| s.date.days_until(row.date).abs() <= REISSUE_DAYS)
            .filter(|s| similar_description(&s.raw_description, &row.raw_description))
            .collect();
        candidates.sort_by_key(oldest);
        match candidates.as_slice() {
            [] => {}
            [only] => {
                taken.insert(only.id);
                matches[i] = Match::Reissued(only.id);
                continue;
            }
            [first, ..] => {
                matches[i] = Match::PossibleDuplicate(first.id);
                continue;
            }
        }

        matches[i] = adopt(row, stored, &taken);
        if let Match::Adopts(id) = matches[i] {
            taken.insert(id);
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> Date {
        text.parse().unwrap()
    }

    fn incoming(id: &str, day: &str, minor: i64, pending: bool, text: &str) -> SyncedRow {
        SyncedRow {
            external_id: id.to_owned(),
            date: date(day),
            amount: Money::from_minor(minor),
            pending,
            raw_description: text.to_owned(),
            payee_text: text.to_owned(),
        }
    }

    fn stored(id: i64, external: Option<&str>, day: &str, minor: i64, text: &str) -> StoredRow {
        StoredRow {
            id,
            external_id: external.map(str::to_owned),
            date: date(day),
            amount: Money::from_minor(minor),
            pending: false,
            raw_description: text.to_owned(),
            imported: false,
            manual: false,
        }
    }

    fn pending(id: i64, external: &str, day: &str, minor: i64, text: &str) -> StoredRow {
        StoredRow {
            pending: true,
            ..stored(id, Some(external), day, minor, text)
        }
    }

    fn imported(id: i64, day: &str, minor: i64, text: &str) -> StoredRow {
        StoredRow {
            imported: true,
            ..stored(id, None, day, minor, text)
        }
    }

    #[test]
    fn similar_descriptions() {
        // One starts with the other, after normalization.
        assert!(similar_description(
            "DEMO BISTRO",
            "Demo Bistro #4411 Springfield"
        ));
        assert!(similar_description("SQ *DEMO CAFE", "sq demo cafe"));
        // First word and first 8 characters shared.
        assert!(similar_description(
            "DEMOGROCER 12 MAIN",
            "DEMOGROCER 13 ELM"
        ));
        assert!(similar_description("DEMO GROCER 12", "DEMO GROCERY STORE"));
        // First word differs, or fewer than 8 characters shared.
        assert!(!similar_description("DEMO GROCER", "DEMO FUEL STOP"));
        assert!(!similar_description("DEMOGROC A", "DEMOGROX B"));
        assert!(!similar_description("CAFE DEMO", "DEMO CAFE"));
        assert!(!similar_description("ABC DEF", "ABC DEG"));
        // Nothing to compare.
        assert!(!similar_description("", "DEMO"));
        assert!(!similar_description("***", "***"));
    }

    #[test]
    fn a_tip_posts_up_to_a_quarter_more() {
        let m = Money::from_minor;
        assert!(posts_for(m(-4000), m(-4000)));
        assert!(posts_for(m(-5000), m(-4000)));
        assert!(!posts_for(m(-5001), m(-4000)));
        assert!(!posts_for(m(-3999), m(-4000)));
        assert!(!posts_for(m(4000), m(-4000)));
        assert!(posts_for(m(1250), m(1000)));
        assert!(posts_for(m(0), m(0)));
    }

    #[test]
    fn the_same_id_matches_first_whatever_else_changed() {
        let rows = [incoming("T1", "2026-10-27", -500, false, "DEMO CAFE")];
        let kept = [
            pending(7, "T1", "2026-10-26", -450, "DEMO"),
            imported(8, "2026-10-27", -500, "DEMO CAFE"),
        ];
        assert_eq!(match_rows(&rows, &kept), [Match::SameId(7)]);
    }

    #[test]
    fn a_pending_row_posts_under_a_new_id_with_a_tip() {
        let rows = [incoming(
            "T2",
            "2026-10-27",
            -4800,
            false,
            "DEMO BISTRO 4411",
        )];
        let kept = [pending(7, "T1", "2026-10-26", -4000, "DEMO BISTRO")];
        assert_eq!(match_rows(&rows, &kept), [Match::Posts(7)]);
    }

    #[test]
    fn a_pending_row_still_in_the_response_or_out_of_reach_isn_t_posted() {
        let kept = [pending(7, "T1", "2026-10-20", -4000, "DEMO BISTRO")];
        // Its own id is still here: it hasn't posted.
        let rows = [
            incoming("T1", "2026-10-20", -4000, true, "DEMO BISTRO"),
            incoming("T2", "2026-10-21", -4000, false, "DEMO BISTRO"),
        ];
        assert_eq!(match_rows(&rows, &kept), [Match::SameId(7), Match::New]);
        // More than 7 days later, before the pending date, too large, or another text.
        for row in [
            incoming("T2", "2026-10-28", -4000, false, "DEMO BISTRO"),
            incoming("T2", "2026-10-19", -4000, false, "DEMO BISTRO"),
            incoming("T2", "2026-10-21", -5100, false, "DEMO BISTRO"),
            incoming("T2", "2026-10-21", -4000, false, "OTHER PLACE"),
        ] {
            assert_eq!(
                match_rows(std::slice::from_ref(&row), &kept),
                [Match::New],
                "{row:?}"
            );
        }
        // Seven days later is still in reach.
        let row = incoming("T2", "2026-10-27", -4000, false, "DEMO BISTRO");
        assert_eq!(match_rows(&[row], &kept), [Match::Posts(7)]);
    }

    #[test]
    fn several_pending_rows_pair_oldest_first_on_exact_amounts() {
        let kept = [
            pending(9, "P3", "2026-10-22", -1000, "DEMO MARKET"),
            pending(7, "P1", "2026-10-20", -1000, "DEMO MARKET"),
            pending(8, "P2", "2026-10-21", -1100, "DEMO MARKET"),
        ];
        let rows = [
            incoming("N2", "2026-10-23", -1000, false, "DEMO MARKET"),
            incoming("N1", "2026-10-23", -1000, false, "DEMO MARKET"),
            incoming("N3", "2026-10-23", -1200, false, "DEMO MARKET"),
        ];
        // N1 and N2 pair with P1 and P3 in order; N3 sees P2 alone and posts it with a tip.
        assert_eq!(
            match_rows(&rows, &kept),
            [Match::Posts(9), Match::Posts(7), Match::Posts(8)]
        );
        // Several candidates, none exact: inserted, and the pending rows retire on their own.
        let rows = [incoming("N1", "2026-10-23", -1050, false, "DEMO MARKET")];
        assert_eq!(match_rows(&rows, &kept), [Match::New]);
    }

    #[test]
    fn a_re_issued_id_moves_onto_its_row() {
        let kept = [stored(
            7,
            Some("OLD"),
            "2026-10-20",
            -450,
            "DEMO COFFEE ROASTERS",
        )];
        for day in ["2026-10-19", "2026-10-20", "2026-10-21"] {
            let rows = [incoming("NEW", day, -450, false, "DEMO COFFEE ROASTERS")];
            assert_eq!(match_rows(&rows, &kept), [Match::Reissued(7)], "{day}");
        }
        // Two days off, another amount, its own id still sent, or a pending row: not it.
        for rows in [
            vec![incoming(
                "NEW",
                "2026-10-22",
                -450,
                false,
                "DEMO COFFEE ROASTERS",
            )],
            vec![incoming(
                "NEW",
                "2026-10-20",
                -451,
                false,
                "DEMO COFFEE ROASTERS",
            )],
            vec![
                incoming("OLD", "2026-10-20", -450, false, "DEMO COFFEE ROASTERS"),
                incoming("NEW", "2026-10-20", -450, false, "DEMO COFFEE ROASTERS"),
            ],
            vec![incoming(
                "NEW",
                "2026-10-20",
                -450,
                true,
                "DEMO COFFEE ROASTERS",
            )],
        ] {
            assert_eq!(
                match_rows(&rows, &kept).last(),
                Some(&Match::New),
                "{rows:?}"
            );
        }
    }

    #[test]
    fn two_rows_a_re_issued_id_could_be_make_a_possible_duplicate() {
        let kept = [
            stored(8, Some("B"), "2026-10-21", -450, "DEMO COFFEE ROASTERS"),
            stored(7, Some("A"), "2026-10-20", -450, "DEMO COFFEE ROASTERS"),
        ];
        let rows = [
            incoming("C", "2026-10-20", -450, false, "DEMO COFFEE ROASTERS"),
            incoming("D", "2026-10-21", -450, false, "DEMO COFFEE ROASTERS"),
        ];
        assert_eq!(
            match_rows(&rows, &kept),
            [Match::PossibleDuplicate(7), Match::PossibleDuplicate(7)]
        );
    }

    #[test]
    fn an_imported_row_without_a_bank_id_is_adopted() {
        let kept = [
            imported(7, "2026-10-17", -2000, "Demo Books"),
            // Has a bank id: never adopted by step 4 (and two days off for step 3).
            StoredRow {
                imported: true,
                ..stored(8, Some("ACTUAL-1"), "2026-10-22", -2000, "DEMO BOOKS")
            },
        ];
        let rows = [incoming("T1", "2026-10-20", -2000, false, "DEMO BOOKS 77")];
        assert_eq!(match_rows(&rows, &kept), [Match::Adopts(7)]);
        // Four days off, or another amount: inserted.
        let rows = [incoming("T1", "2026-10-21", -2000, false, "DEMO BOOKS")];
        assert_eq!(match_rows(&rows, &kept[..1]), [Match::New]);
        let rows = [incoming("T1", "2026-10-20", -2001, false, "DEMO BOOKS")];
        assert_eq!(match_rows(&rows, &kept[..1]), [Match::New]);
    }

    #[test]
    fn identical_imported_rows_are_counted_and_a_tie_is_flagged() {
        let kept = [
            imported(7, "2026-10-20", -300, "DEMO TRANSIT"),
            imported(8, "2026-10-20", -300, "DEMO TRANSIT"),
        ];
        let rows = [
            incoming("A", "2026-10-20", -300, false, "DEMO TRANSIT"),
            incoming("B", "2026-10-20", -300, false, "DEMO TRANSIT"),
            incoming("C", "2026-10-20", -300, false, "DEMO TRANSIT"),
        ];
        assert_eq!(
            match_rows(&rows, &kept),
            [Match::Adopts(7), Match::Adopts(8), Match::New]
        );

        // The nearest date wins; then a similar description.
        let kept = [
            imported(7, "2026-10-18", -300, "DEMO TRANSIT"),
            imported(8, "2026-10-21", -300, "DEMO TRANSIT"),
            imported(9, "2026-10-21", -300, "SOMETHING ELSE"),
        ];
        let rows = [incoming("A", "2026-10-20", -300, false, "DEMO TRANSIT")];
        assert_eq!(match_rows(&rows, &kept), [Match::Adopts(8)]);

        // Equally near and equally similar, but not the same row: a tie.
        let kept = [
            imported(7, "2026-10-19", -300, "DEMO TRANSIT"),
            imported(8, "2026-10-21", -300, "DEMO TRANSIT"),
        ];
        assert_eq!(match_rows(&rows, &kept), [Match::PossibleDuplicate(7)]);
    }

    fn manual(id: i64, day: &str, minor: i64) -> StoredRow {
        StoredRow {
            manual: true,
            ..stored(id, None, day, minor, "")
        }
    }

    #[test]
    fn a_manual_row_is_adopted_from_3_days_before_to_14_after_its_date() {
        let kept = [manual(7, "2026-10-10", -12000)];
        for day in ["2026-10-07", "2026-10-10", "2026-10-24"] {
            let rows = [incoming("T1", day, -12000, false, "CHECK 1042")];
            assert_eq!(match_rows(&rows, &kept), [Match::Adopts(7)], "{day}");
        }
        // A day further either way, another amount, or a row with a bank id: inserted.
        for day in ["2026-10-06", "2026-10-25"] {
            let rows = [incoming("T1", day, -12000, false, "CHECK 1042")];
            assert_eq!(match_rows(&rows, &kept), [Match::New], "{day}");
        }
        let rows = [incoming("T1", "2026-10-11", 12000, false, "CHECK 1042")];
        assert_eq!(match_rows(&rows, &kept), [Match::New]);
        let synced = [StoredRow {
            external_id: Some("OLD".to_owned()),
            ..manual(7, "2026-10-10", -12000)
        }];
        let rows = [incoming("T1", "2026-10-11", -12000, false, "CHECK 1042")];
        assert_eq!(match_rows(&rows, &synced), [Match::New]);
        // A pending bank row adopts it too.
        let rows = [incoming("T1", "2026-10-11", -12000, true, "CHECK 1042")];
        assert_eq!(match_rows(&rows, &kept), [Match::Adopts(7)]);
    }

    #[test]
    fn two_manual_rows_of_the_amount_make_a_possible_duplicate_and_one_row_takes_one() {
        let kept = [
            manual(8, "2026-10-12", -2500),
            manual(7, "2026-10-11", -2500),
        ];
        let rows = [incoming("T1", "2026-10-12", -2500, false, "DEMO PAY APP")];
        assert_eq!(match_rows(&rows, &kept), [Match::PossibleDuplicate(7)]);
        // Each bank row takes one manual row at most.
        let kept = [manual(7, "2026-10-11", -2500)];
        let rows = [
            incoming("T1", "2026-10-12", -2500, false, "DEMO PAY APP"),
            incoming("T2", "2026-10-13", -2500, false, "DEMO PAY APP"),
        ];
        assert_eq!(match_rows(&rows, &kept), [Match::Adopts(7), Match::New]);
    }

    #[test]
    fn a_bank_amount_up_to_a_quarter_larger_is_a_possible_manual_match() {
        let kept = [manual(7, "2026-10-11", -4000)];
        let rows = [incoming("T1", "2026-10-12", -4600, false, "DEMO SITTER")];
        assert_eq!(match_rows(&rows, &kept), [Match::PossibleManualMatch(7)]);
        let rows = [incoming("T1", "2026-10-12", -5000, false, "DEMO SITTER")];
        assert_eq!(match_rows(&rows, &kept), [Match::PossibleManualMatch(7)]);
        // More than a quarter, or smaller: inserted.
        for minor in [-5001, -3900] {
            let rows = [incoming("T1", "2026-10-12", minor, false, "DEMO SITTER")];
            assert_eq!(match_rows(&rows, &kept), [Match::New], "{minor}");
        }
        // Two candidates: no guess.
        let kept = [
            manual(7, "2026-10-11", -4000),
            manual(8, "2026-10-12", -4200),
        ];
        let rows = [incoming("T1", "2026-10-12", -4600, false, "DEMO SITTER")];
        assert_eq!(match_rows(&rows, &kept), [Match::New]);
    }

    #[test]
    fn an_imported_row_is_adopted_before_a_manual_one() {
        let kept = [
            manual(7, "2026-10-11", -900),
            imported(8, "2026-10-12", -900, "DEMO DELI"),
        ];
        let rows = [incoming("T1", "2026-10-12", -900, false, "DEMO DELI")];
        assert_eq!(match_rows(&rows, &kept), [Match::Adopts(8)]);
    }

    #[test]
    fn the_earliest_candidate_is_two_weeks_back() {
        assert_eq!(
            earliest_candidate(date("2026-10-28")),
            Some(date("2026-10-14"))
        );
    }
}
