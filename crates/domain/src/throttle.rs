//! The login throttle: 5 failed attempts within 15 minutes pause that key for 15 minutes.
//! Fixed in code, not configurable (docs/spec/foundations.md#auth). The key is the username as
//! typed, or `setup` for the setup code.

use jiff::{SignedDuration, Timestamp};

/// How many failures within [`WINDOW`] start a pause.
pub const MAX_FAILURES: usize = 5;
pub const WINDOW: SignedDuration = SignedDuration::from_mins(15);
pub const PAUSE: SignedDuration = SignedDuration::from_mins(15);

/// Attempts are refused until `until`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Too many attempts. Try again in {} minute{}.", self.minutes_left(), if self.minutes_left() == 1 { "" } else { "s" })]
pub struct Paused {
    pub until: Timestamp,
    pub now: Timestamp,
}

impl Paused {
    /// Whole minutes until the pause ends, rounded up: never 0 while paused.
    #[must_use]
    pub fn minutes_left(&self) -> i64 {
        let secs = self.until.duration_since(self.now).as_secs();
        (secs + 59) / 60
    }
}

/// Whether an attempt at `now` may go ahead, given the key's most recent failures, newest first
/// (only the first [`MAX_FAILURES`] matter). Refused attempts aren't failures, so a pause doesn't
/// extend itself.
///
/// # Errors
/// [`Paused`] while the last [`MAX_FAILURES`] failures fell within [`WINDOW`] of each other and
/// the newest is less than [`PAUSE`] ago.
pub fn check(recent_failures: &[Timestamp], now: Timestamp) -> Result<(), Paused> {
    let (Some(newest), Some(oldest)) = (
        recent_failures.first(),
        recent_failures.get(MAX_FAILURES - 1),
    ) else {
        return Ok(());
    };
    let until = *newest + PAUSE;
    if newest.duration_since(*oldest) < WINDOW && now < until {
        Err(Paused { until, now })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minute: i64) -> Timestamp {
        Timestamp::UNIX_EPOCH + SignedDuration::from_mins(minute)
    }

    /// Failures at these minutes, newest first, as the database hands them over.
    fn failures(minutes: &[i64]) -> Vec<Timestamp> {
        let mut times: Vec<Timestamp> = minutes.iter().copied().map(at).collect();
        times.sort_unstable_by(|a, b| b.cmp(a));
        times
    }

    #[test]
    fn four_failures_never_pause() {
        assert_eq!(check(&failures(&[0, 1, 2, 3]), at(3)), Ok(()));
        assert_eq!(check(&[], at(0)), Ok(()));
    }

    #[test]
    fn five_within_fifteen_minutes_pause_for_fifteen_more() {
        let five = failures(&[0, 1, 2, 3, 4]);
        let paused = check(&five, at(4)).unwrap_err();
        assert_eq!(paused.until, at(19));
        assert_eq!(paused.minutes_left(), 15);
        assert!(check(&five, at(18)).is_err());
        assert_eq!(check(&five, at(19)), Ok(()));
    }

    #[test]
    fn five_spread_wider_than_fifteen_minutes_dont_pause() {
        assert_eq!(check(&failures(&[0, 4, 8, 12, 15]), at(15)), Ok(()));
        assert!(check(&failures(&[1, 4, 8, 12, 15]), at(15)).is_err());
    }

    #[test]
    fn only_the_newest_five_count() {
        assert_eq!(
            check(&failures(&[0, 1, 2, 3, 20, 40, 60, 80]), at(80)),
            Ok(())
        );
    }

    #[test]
    fn a_failure_after_the_pause_starts_a_fresh_count() {
        assert_eq!(check(&failures(&[0, 1, 2, 3, 4, 19]), at(19)), Ok(()));
    }

    #[test]
    fn says_how_long_is_left_rounded_up() {
        let five = failures(&[0, 1, 2, 3, 4]);
        let left = |now: Timestamp| check(&five, now).unwrap_err();
        let almost_done = at(19) - SignedDuration::from_secs(1);
        assert_eq!(left(almost_done).minutes_left(), 1);
        assert_eq!(
            left(almost_done).to_string(),
            "Too many attempts. Try again in 1 minute."
        );
        assert_eq!(
            left(at(5)).to_string(),
            "Too many attempts. Try again in 14 minutes."
        );
    }
}
