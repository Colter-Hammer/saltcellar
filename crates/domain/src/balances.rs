//! Balances (docs/spec/m2-bank-data.md#balances): which bank balances become snapshots, the
//! sign-bug flip, which app balance the bank's is compared with, drift, and when drift is flagged.
//! The database runs them; they are pure here so the edges can be tested one by one.

use crate::accounts::AccountType;
use crate::{Date, Money};

/// Whether a bank balance refreshed at `balance_date` (epoch seconds) is written as a snapshot:
/// only when it moved forward past the newest provider snapshot's (`latest`). A run where
/// `balance-date` didn't move writes nothing and doesn't count toward drift.
#[must_use]
pub fn moved_forward(latest: Option<i64>, balance_date: i64) -> bool {
    latest.is_none_or(|latest| balance_date > latest)
}

/// What an account is, as far as the sign rule needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignCheck {
    /// An account with transactions, and the app's posted-only balance to compare with.
    Rows { app_posted: Money },
    /// A balances-only account of type `kind`.
    BalancesOnly { kind: AccountType },
}

/// The bank balance to save, and whether it was flipped (`sign_corrected`).
///
/// The provider sometimes reports a balance with the wrong sign while the rows are right. On an
/// account with transactions, a balance whose sign differs from the app's posted-only balance,
/// where the flipped value is closer to it, is flipped, unless both are within one major unit
/// (`unit`, in minor units: 100 for two decimal places) of zero *(agent: the near-zero crossover,
/// a card paid off with a refund pending, is left alone)*. A zero on either side has no sign, so
/// it is never flipped. On a balances-only loan, a positive balance is flipped *(agent)*.
#[must_use]
pub fn correct_sign(bank: Money, check: SignCheck, unit: Money) -> (Money, bool) {
    let Some(flipped) = bank.checked_neg() else {
        return (bank, false);
    };
    let flip = match check {
        SignCheck::Rows { app_posted } => {
            let opposite = bank.minor().signum() * app_posted.minor().signum() < 0;
            let near_zero = within(bank, unit) && within(app_posted, unit);
            let closer = distance(flipped, app_posted) < distance(bank, app_posted);
            opposite && closer && !near_zero
        }
        SignCheck::BalancesOnly { kind } => kind == AccountType::Loan && bank.minor() > 0,
    };
    if flip { (flipped, true) } else { (bank, false) }
}

fn within(amount: Money, unit: Money) -> bool {
    amount.minor().unsigned_abs() <= unit.minor().unsigned_abs()
}

fn distance(a: Money, b: Money) -> u128 {
    (i128::from(a.minor()) - i128::from(b.minor())).unsigned_abs()
}

/// The app's two balances beside a snapshot: opening + posted rows + adjustments, and the same
/// with pending rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppBalances {
    pub posted: Money,
    pub with_pending: Money,
}

/// What `accounts.balance_includes_pending` becomes after a snapshot of `bank`: once learned it
/// stays; while unknown, it is learned the first time the bank's balance matches exactly one of
/// the two app balances *(agent)*.
#[must_use]
pub fn learn_includes_pending(known: Option<bool>, bank: Money, app: AppBalances) -> Option<bool> {
    if known.is_some() {
        return known;
    }
    match (bank == app.posted, bank == app.with_pending) {
        (true, false) => Some(false),
        (false, true) => Some(true),
        _ => None,
    }
}

/// Drift, the bank's balance less the app's: against the posted-only balance unless the account
/// is known to count pending rows. While that isn't known, a bank balance matching either app
/// balance is no drift, so the account is flagged only when neither matches. `None` if the
/// difference doesn't fit.
#[must_use]
pub fn drift(bank: Money, app: AppBalances, includes_pending: Option<bool>) -> Option<Money> {
    match includes_pending {
        Some(true) => bank.checked_sub(app.with_pending),
        None if bank == app.posted || bank == app.with_pending => Some(Money::ZERO),
        Some(false) | None => bank.checked_sub(app.posted),
    }
}

/// A drift flag: drift that held at the same non-zero value for the last two snapshots or more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flag {
    pub drift: Money,
    /// The index of the snapshot where drift last stepped to this value: its date is the flag's
    /// "since", and the rows its run touched are the suspect rows.
    pub stepped_at: usize,
}

/// How many consecutive snapshots at the same drift make a flag.
pub const FLAG_AFTER: usize = 2;

/// The drift flag over `drifts`, each snapshot's drift oldest first; `None` when the newest is
/// zero or hasn't held for [`FLAG_AFTER`] snapshots. The app never adjusts on its own.
#[must_use]
pub fn flag(drifts: &[Money]) -> Option<Flag> {
    let last = *drifts.last()?;
    if last.is_zero() {
        return None;
    }
    let held = drifts.iter().rev().take_while(|d| **d == last).count();
    (held >= FLAG_AFTER).then(|| Flag {
        drift: last,
        stepped_at: drifts.len() - held,
    })
}

/// The opening balance a first sync derives: the bank's balance less the account's posted rows
/// (`posted`, their total) and adjustments, dated the day before the earliest row, or the day
/// before the snapshot's date (`as_of`) without one *(agent)*. `None` if it doesn't fit.
#[must_use]
pub fn first_opening(
    bank: Money,
    posted: Money,
    earliest: Option<Date>,
    as_of: Date,
) -> Option<crate::accounts::Opening> {
    Some(crate::accounts::Opening {
        balance: bank.checked_sub(posted)?,
        date: earliest.unwrap_or(as_of).previous_day()?,
    })
}

/// The opening balance once older rows arrive (an import, files, a back-fill): `older`, the total
/// of the posted rows dated on or before its date, comes off it, and it is dated the day before
/// the earliest row, so every app balance, and with it drift, stays as it was. `None` if it
/// doesn't fit.
#[must_use]
pub fn moved_back(
    opening: crate::accounts::Opening,
    older: Money,
    earliest: Date,
) -> Option<crate::accounts::Opening> {
    Some(crate::accounts::Opening {
        balance: opening.balance.checked_sub(older)?,
        date: earliest.previous_day()?.min(opening.date),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::Opening;

    const CENT: Money = Money::from_minor(100);

    fn m(minor: i64) -> Money {
        Money::from_minor(minor)
    }

    fn rows(app_posted: i64) -> SignCheck {
        SignCheck::Rows {
            app_posted: m(app_posted),
        }
    }

    fn date(text: &str) -> Date {
        text.parse().unwrap()
    }

    #[test]
    fn a_snapshot_is_written_only_when_balance_date_moves_forward() {
        assert!(moved_forward(None, 1_000));
        assert!(moved_forward(Some(999), 1_000));
        // Unmoved, or older than the newest: nothing.
        assert!(!moved_forward(Some(1_000), 1_000));
        assert!(!moved_forward(Some(1_001), 1_000));
    }

    #[test]
    fn a_balance_with_the_wrong_sign_is_flipped_when_the_flip_is_closer() {
        // A card owing $229.00 reported as +229.00.
        assert_eq!(
            correct_sign(m(22_900), rows(-22_900), CENT),
            (m(-22_900), true)
        );
        // Not exactly equal, still closer flipped.
        assert_eq!(
            correct_sign(m(22_900), rows(-20_000), CENT),
            (m(-22_900), true)
        );
        // And the other way round: an account in credit reported negative.
        assert_eq!(correct_sign(m(-5_000), rows(4_800), CENT), (m(5_000), true));
    }

    #[test]
    fn a_balance_with_the_same_sign_is_left_alone() {
        assert_eq!(
            correct_sign(m(-22_900), rows(-22_900), CENT),
            (m(-22_900), false)
        );
        assert_eq!(correct_sign(m(500), rows(80_000), CENT), (m(500), false));
    }

    #[test]
    fn a_card_paid_past_zero_is_flipped_too() {
        // With opposite signs the flip is always the closer one, so +$100.00 against -$10.00
        // flips: the rule trusts the rows over the balance's sign.
        assert_eq!(
            correct_sign(m(10_000), rows(-1_000), CENT),
            (m(-10_000), true)
        );
    }

    #[test]
    fn the_near_zero_crossover_is_left_alone() {
        // A card paid off with a refund: +$0.50 at the bank, -$0.40 in the app.
        assert_eq!(correct_sign(m(50), rows(-40), CENT), (m(50), false));
        // Exactly one major unit on both sides still counts as near zero.
        assert_eq!(correct_sign(m(100), rows(-100), CENT), (m(100), false));
        // One side past a unit: the flip rule applies again.
        assert_eq!(correct_sign(m(101), rows(-100), CENT), (m(-101), true));
        assert_eq!(correct_sign(m(100), rows(-101), CENT), (m(-100), true));
        // In a currency without decimals the unit is 1.
        assert_eq!(correct_sign(m(1), rows(-1), m(1)), (m(1), false));
        assert_eq!(correct_sign(m(2), rows(-2), m(1)), (m(-2), true));
    }

    #[test]
    fn a_zero_on_either_side_is_never_flipped() {
        assert_eq!(correct_sign(m(0), rows(-5_000), CENT), (m(0), false));
        assert_eq!(correct_sign(m(5_000), rows(0), CENT), (m(5_000), false));
    }

    #[test]
    fn a_balances_only_loan_with_a_positive_balance_is_flipped() {
        let loan = SignCheck::BalancesOnly {
            kind: AccountType::Loan,
        };
        assert_eq!(
            correct_sign(m(1_500_000), loan, CENT),
            (m(-1_500_000), true)
        );
        assert_eq!(
            correct_sign(m(-1_500_000), loan, CENT),
            (m(-1_500_000), false)
        );
        // Other balances-only accounts are left as reported.
        let investment = SignCheck::BalancesOnly {
            kind: AccountType::Investment,
        };
        assert_eq!(
            correct_sign(m(-4_000), investment, CENT),
            (m(-4_000), false)
        );
        assert_eq!(correct_sign(m(4_000), investment, CENT), (m(4_000), false));
    }

    #[test]
    fn i64_min_is_never_flipped() {
        assert_eq!(
            correct_sign(m(i64::MIN), rows(5), CENT),
            (m(i64::MIN), false)
        );
    }

    fn app(posted: i64, with_pending: i64) -> AppBalances {
        AppBalances {
            posted: m(posted),
            with_pending: m(with_pending),
        }
    }

    #[test]
    fn includes_pending_is_learned_from_exactly_one_match_and_then_kept() {
        assert_eq!(
            learn_includes_pending(None, m(100), app(100, 80)),
            Some(false)
        );
        assert_eq!(
            learn_includes_pending(None, m(80), app(100, 80)),
            Some(true)
        );
        // Both match (no pending rows), or neither: still unknown.
        assert_eq!(learn_includes_pending(None, m(100), app(100, 100)), None);
        assert_eq!(learn_includes_pending(None, m(90), app(100, 80)), None);
        // Once learned it stays.
        assert_eq!(
            learn_includes_pending(Some(false), m(80), app(100, 80)),
            Some(false)
        );
        assert_eq!(
            learn_includes_pending(Some(true), m(100), app(100, 80)),
            Some(true)
        );
    }

    #[test]
    fn drift_compares_with_the_posted_only_balance_unless_pending_is_learned() {
        assert_eq!(drift(m(90), app(100, 80), Some(false)), Some(m(-10)));
        assert_eq!(drift(m(90), app(100, 80), Some(true)), Some(m(10)));
        // Unknown: either match is no drift; neither is drift against posted-only.
        assert_eq!(drift(m(100), app(100, 80), None), Some(m(0)));
        assert_eq!(drift(m(80), app(100, 80), None), Some(m(0)));
        assert_eq!(drift(m(90), app(100, 80), None), Some(m(-10)));
        assert_eq!(drift(m(i64::MIN), app(1, 1), Some(false)), None);
    }

    #[test]
    fn drift_is_flagged_after_two_snapshots_at_the_same_value() {
        assert_eq!(flag(&[]), None);
        assert_eq!(flag(&[m(0)]), None);
        // One snapshot off: not yet.
        assert_eq!(flag(&[m(0), m(-950)]), None);
        // Two in a row: flagged, since the first of them.
        assert_eq!(
            flag(&[m(0), m(-950), m(-950)]),
            Some(Flag {
                drift: m(-950),
                stepped_at: 1
            })
        );
        // It keeps its date while it holds.
        assert_eq!(
            flag(&[m(0), m(-950), m(-950), m(-950)]),
            Some(Flag {
                drift: m(-950),
                stepped_at: 1
            })
        );
        // A new value starts over; the date is when drift last stepped.
        assert_eq!(flag(&[m(0), m(-950), m(-950), m(-400)]), None);
        assert_eq!(
            flag(&[m(0), m(-950), m(-950), m(-400), m(-400)]),
            Some(Flag {
                drift: m(-400),
                stepped_at: 3
            })
        );
        // Back to zero clears it.
        assert_eq!(flag(&[m(0), m(-950), m(-950), m(0)]), None);
        // Drift from the very first snapshot.
        assert_eq!(
            flag(&[m(5), m(5)]),
            Some(Flag {
                drift: m(5),
                stepped_at: 0
            })
        );
    }

    #[test]
    fn a_first_sync_derives_the_opening_from_the_bank_balance_and_its_rows() {
        assert_eq!(
            first_opening(
                m(115_758),
                m(104_958),
                Some(date("2026-10-08")),
                date("2026-10-28")
            ),
            Some(Opening {
                balance: m(10_800),
                date: date("2026-10-07")
            })
        );
        // No rows: the whole balance, dated the day before the snapshot.
        assert_eq!(
            first_opening(m(500_318), m(0), None, date("2026-10-28")),
            Some(Opening {
                balance: m(500_318),
                date: date("2026-10-27")
            })
        );
        assert_eq!(
            first_opening(m(i64::MIN), m(1), None, date("2026-10-28")),
            None
        );
    }

    #[test]
    fn older_rows_move_the_opening_back_and_keep_the_app_balance() {
        let opening = Opening {
            balance: m(10_800),
            date: date("2026-10-07"),
        };
        // Two older rows, -30.00 and +12.00, the earliest on Sep 1.
        let moved = moved_back(opening, m(-1_800), date("2026-09-01")).unwrap();
        assert_eq!(
            moved,
            Opening {
                balance: m(12_600),
                date: date("2026-08-31")
            }
        );
        // The opening plus every row is what it was: 108.00 + (-18.00 older) == 126.00 - 18.00.
        assert_eq!(
            moved.balance.checked_add(m(-1_800)),
            opening.balance.checked_add(m(0))
        );
        // A row on the opening date itself is older history too; the date never moves forward.
        assert_eq!(
            moved_back(opening, m(-500), date("2026-10-07")).unwrap(),
            Opening {
                balance: m(11_300),
                date: date("2026-10-06")
            }
        );
        assert_eq!(
            moved_back(opening, m(0), date("2026-10-20")).unwrap().date,
            date("2026-10-07")
        );
    }
}
