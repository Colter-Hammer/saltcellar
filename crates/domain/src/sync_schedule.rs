//! When bank syncs run (docs/spec/m2-bank-data.md#sync-runs): the daily time, the request budget
//! with the request held back for the scheduled run, the Sync now cooldown, retries after a
//! transient failure, and the catch-up after a restart. Pure: every rule takes the time it is
//! asked at, so tests move a fake clock.

use jiff::civil::Time;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp, ToSpan};

/// Bridge requests the app makes in any rolling 24 hours, half the Bridge's own limit.
pub const REQUEST_CAP: u32 = 12;
/// The window the cap counts over.
pub const CAP_WINDOW: SignedDuration = SignedDuration::from_hours(24);
/// How long Sync now waits after a sync.
pub const COOLDOWN: SignedDuration = SignedDuration::from_mins(30);
/// After a transient failure, retries at about these delays from the failure; then the next day.
pub const RETRY_DELAYS: [SignedDuration; 2] =
    [SignedDuration::from_mins(20), SignedDuration::from_hours(2)];
/// A restart catches up when the last successful sync is older than this.
pub const CATCH_UP_AFTER: SignedDuration = SignedDuration::from_hours(20);
/// How long after startup the catch-up runs *(agent)*.
pub const CATCH_UP_DELAY: SignedDuration = SignedDuration::from_mins(3);
/// How late the scheduled run may still start, if the app was busy or just started at its time
/// *(agent)*. Later than this, only a restart's catch-up makes up for it.
pub const SCHEDULE_GRACE: SignedDuration = SignedDuration::from_hours(1);
/// How long the run history is kept.
pub const RUNS_KEPT: SignedDuration = SignedDuration::from_hours(90 * 24);
/// How long kept raw bank responses are kept.
pub const RAW_RESPONSES_KEPT: SignedDuration = SignedDuration::from_hours(7 * 24);

/// The time of day the daily sync runs, in the household timezone (the `sync.time` setting).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncTime(Time);

impl Default for SyncTime {
    /// 06:13: after the provider's overnight refresh, and off the hour as the Bridge asks.
    fn default() -> Self {
        SyncTime(Time::constant(6, 13, 0, 0))
    }
}

/// A sync time that isn't `HH:MM`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("isn't a time of day like 06:13")]
pub struct BadSyncTime;

impl SyncTime {
    /// Reads `HH:MM`, 24-hour.
    ///
    /// # Errors
    /// [`BadSyncTime`] for anything else.
    pub fn parse(text: &str) -> Result<SyncTime, BadSyncTime> {
        let (hour, minute) = text.trim().split_once(':').ok_or(BadSyncTime)?;
        if hour.len() != 2 || minute.len() != 2 {
            return Err(BadSyncTime);
        }
        let hour: i8 = hour.parse().map_err(|_| BadSyncTime)?;
        let minute: i8 = minute.parse().map_err(|_| BadSyncTime)?;
        Time::new(hour, minute, 0, 0)
            .map(SyncTime)
            .map_err(|_| BadSyncTime)
    }

    /// The latest daily slot at or before `now`, in timezone `tz`. A time a clock change skips
    /// runs just after it; one it repeats runs the first time *(agent)*.
    #[must_use]
    pub fn slot_at_or_before(self, now: Timestamp, tz: &TimeZone) -> Timestamp {
        let today = now.to_zoned(tz.clone()).date();
        let slot = |date: jiff::civil::Date| {
            date.to_datetime(self.0)
                .to_zoned(tz.clone())
                .map_or(now, |zoned| zoned.timestamp())
        };
        let on_today = slot(today);
        if on_today <= now {
            return on_today;
        }
        today.checked_sub(1.day()).map_or(on_today, slot)
    }
}

impl std::fmt::Display for SyncTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02}:{:02}", self.0.hour(), self.0.minute())
    }
}

/// What the scheduled run of `slot` should do at `now`, given when the last scheduled run
/// started: run only inside the grace after the slot, and once.
#[must_use]
pub fn scheduled_due(slot: Timestamp, last_scheduled: Option<Timestamp>, now: Timestamp) -> bool {
    now >= slot && now < slot + SCHEDULE_GRACE && last_scheduled.is_none_or(|last| last < slot)
}

/// A run that spent Bridge requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spent {
    /// When it started.
    pub at: Timestamp,
    pub requests: u32,
}

/// Who asks the budget: the scheduled run may use the request held back for it; nothing else
/// may.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asker {
    Scheduled,
    Other,
}

/// Requests spent in the 24 hours before `now`.
#[must_use]
pub fn requests_used(spent: &[Spent], now: Timestamp) -> u32 {
    spent
        .iter()
        .filter(|s| s.at > now - CAP_WINDOW && s.at <= now)
        .map(|s| s.requests)
        .sum()
}

/// Requests left of the cap at `now`, the held-back one included.
#[must_use]
pub fn requests_left(spent: &[Spent], now: Timestamp) -> u32 {
    REQUEST_CAP.saturating_sub(requests_used(spent, now))
}

/// Whether `asker` may spend `needed` requests at `now`.
#[must_use]
pub fn allows(spent: &[Spent], now: Timestamp, asker: Asker, needed: u32) -> bool {
    let limit = match asker {
        Asker::Scheduled => REQUEST_CAP,
        Asker::Other => REQUEST_CAP - 1,
    };
    requests_used(spent, now) + needed <= limit
}

/// When `asker` may next spend `needed` requests: `None` if it may now, else the moment enough
/// of the spent requests leave the window. `None` too if it never could (it asks for more than
/// the cap).
#[must_use]
pub fn allowed_from(
    spent: &[Spent],
    now: Timestamp,
    asker: Asker,
    needed: u32,
) -> Option<Timestamp> {
    if allows(spent, now, asker, needed) {
        return None;
    }
    let mut leaving: Vec<Timestamp> = spent
        .iter()
        .filter(|s| s.at > now - CAP_WINDOW && s.at <= now)
        .map(|s| s.at + CAP_WINDOW)
        .collect();
    leaving.sort_unstable();
    leaving
        .into_iter()
        .find(|&at| allows(spent, at, asker, needed))
}

/// When Sync now is next allowed after a sync that finished at `last_finished`: `None` if now.
#[must_use]
pub fn cooldown_until(
    last_finished: Option<Timestamp>,
    cooldown: SignedDuration,
    now: Timestamp,
) -> Option<Timestamp> {
    last_finished
        .map(|at| at + cooldown)
        .filter(|&until| until > now)
}

/// Whether an error code is a transient failure that is retried: no answer, a 5xx, the Bridge's
/// `act.failed` and `act.missingdata`, and bank link errors other than `con.auth`. An errlist code
/// with an unknown subcode counts as its prefix.
#[must_use]
pub fn transient(code: &str) -> bool {
    if code == "unreachable" {
        return true;
    }
    if let Ok(status) = code.parse::<u16>() {
        return (500..=599).contains(&status);
    }
    let mut parts = code.split('.');
    match (parts.next(), parts.next()) {
        (Some("act"), Some("failed" | "missingdata")) => true,
        (Some("con"), Some(sub)) => sub != "auth",
        _ => false,
    }
}

/// When retry `attempt` (0 for the first) after a transient failure at `failed_at` runs, off the
/// hour; `None` once the retries are used up.
#[must_use]
pub fn retry_at(failed_at: Timestamp, attempt: usize) -> Option<Timestamp> {
    RETRY_DELAYS
        .get(attempt)
        .map(|&delay| off_the_hour(failed_at + delay))
}

/// When a catch-up runs, for an app that started at `started` and last synced successfully at
/// `last_ok`: `None` if that sync is recent enough.
#[must_use]
pub fn catch_up_at(last_ok: Option<Timestamp>, started: Timestamp) -> Option<Timestamp> {
    let stale = last_ok.is_none_or(|at| started.duration_since(at) > CATCH_UP_AFTER);
    stale.then(|| off_the_hour(started + CATCH_UP_DELAY))
}

/// Moves a time that falls in the first minutes of an hour, when everyone's clients call the
/// Bridge, a few minutes on *(agent)*.
#[must_use]
pub fn off_the_hour(at: Timestamp) -> Timestamp {
    let minute = at.to_zoned(TimeZone::UTC).minute();
    if minute < 3 {
        at + SignedDuration::from_mins(i64::from(7 - minute))
    } else {
        at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    fn mins(n: i64) -> SignedDuration {
        SignedDuration::from_mins(n)
    }

    #[test]
    fn a_sync_time_is_hh_mm() {
        assert_eq!(SyncTime::parse("06:13").unwrap().to_string(), "06:13");
        assert_eq!(SyncTime::parse(" 23:59 ").unwrap().to_string(), "23:59");
        assert_eq!(SyncTime::default().to_string(), "06:13");
        for bad in ["6:13", "24:00", "06:60", "0613", "ab:cd", "", "06:1"] {
            assert_eq!(SyncTime::parse(bad), Err(BadSyncTime), "{bad}");
        }
    }

    #[test]
    fn the_slot_is_the_latest_daily_time_in_the_household_timezone() {
        let tz = TimeZone::get("America/Chicago").unwrap();
        let time = SyncTime::default();
        // 06:13 CDT is 11:13 UTC.
        assert_eq!(
            time.slot_at_or_before(at("2026-10-28T12:00:00Z"), &tz),
            at("2026-10-28T11:13:00Z")
        );
        assert_eq!(
            time.slot_at_or_before(at("2026-10-28T11:13:00Z"), &tz),
            at("2026-10-28T11:13:00Z")
        );
        assert_eq!(
            time.slot_at_or_before(at("2026-10-28T11:12:59Z"), &tz),
            at("2026-10-27T11:13:00Z")
        );
        // After the clocks go back, 06:13 CST is 12:13 UTC.
        assert_eq!(
            time.slot_at_or_before(at("2026-11-02T13:00:00Z"), &tz),
            at("2026-11-02T12:13:00Z")
        );
        // 02:30 doesn't exist the day clocks go forward: it runs at 03:30.
        let spring = SyncTime::parse("02:30").unwrap();
        assert_eq!(
            spring.slot_at_or_before(at("2026-03-08T12:00:00Z"), &tz),
            at("2026-03-08T08:30:00Z")
        );
    }

    #[test]
    fn the_scheduled_run_is_due_once_inside_the_grace() {
        let slot = at("2026-10-28T11:13:00Z");
        assert!(!scheduled_due(slot, None, slot - mins(1)));
        assert!(scheduled_due(slot, None, slot));
        assert!(scheduled_due(
            slot,
            Some(slot - mins(60 * 24)),
            slot + mins(59)
        ));
        assert!(!scheduled_due(slot, None, slot + mins(60)));
        assert!(!scheduled_due(slot, Some(slot), slot + mins(1)));
    }

    fn spent(at_: &str, requests: u32) -> Spent {
        Spent {
            at: at(at_),
            requests,
        }
    }

    #[test]
    fn the_cap_counts_a_rolling_day_and_holds_one_for_the_scheduled_run() {
        let now = at("2026-10-28T15:00:00Z");
        let mut runs = vec![spent("2026-10-27T14:00:00Z", 5)]; // over 24 h ago
        for hour in 4..14 {
            runs.push(spent(&format!("2026-10-28T{hour:02}:00:00Z"), 1));
        }
        assert_eq!(requests_used(&runs, now), 10);
        assert_eq!(requests_left(&runs, now), 2);
        // Sync now may take the eleventh, never the twelfth.
        assert!(allows(&runs, now, Asker::Other, 1));
        assert!(!allows(&runs, now, Asker::Other, 2));
        runs.push(spent("2026-10-28T14:30:00Z", 1));
        assert!(!allows(&runs, now, Asker::Other, 1));
        assert!(allows(&runs, now, Asker::Scheduled, 1));
        // The 04:00 run leaves the window at 04:00 tomorrow.
        assert_eq!(
            allowed_from(&runs, now, Asker::Other, 1),
            Some(at("2026-10-29T04:00:00Z"))
        );
        assert_eq!(
            allowed_from(&runs, now, Asker::Other, 2),
            Some(at("2026-10-29T05:00:00Z"))
        );
        assert_eq!(allowed_from(&runs, now, Asker::Scheduled, 1), None);
        runs.push(spent("2026-10-28T14:40:00Z", 1));
        assert_eq!(requests_left(&runs, now), 0);
        assert!(!allows(&runs, now, Asker::Scheduled, 1));
        assert_eq!(allowed_from(&runs, now, Asker::Other, 13), None);
    }

    #[test]
    fn sync_now_cools_down_after_a_sync() {
        let done = at("2026-10-28T15:00:00Z");
        assert_eq!(cooldown_until(None, COOLDOWN, done), None);
        assert_eq!(
            cooldown_until(Some(done), COOLDOWN, done + mins(12)),
            Some(done + mins(30))
        );
        assert_eq!(cooldown_until(Some(done), COOLDOWN, done + mins(30)), None);
        assert_eq!(cooldown_until(Some(done), SignedDuration::ZERO, done), None);
    }

    #[test]
    fn only_transient_failures_are_retried() {
        for code in [
            "unreachable",
            "500",
            "503",
            "act.failed",
            "act.missingdata",
            "act.missingdata.partial",
            "con.timeout",
        ] {
            assert!(transient(code), "{code}");
        }
        for code in [
            "403",
            "402",
            "404",
            "con.auth",
            "gen.auth",
            "gen.api",
            "absent",
            "bad_amount",
            "bad_answer",
            "act.other",
        ] {
            assert!(!transient(code), "{code}");
        }
    }

    #[test]
    fn retries_come_at_twenty_minutes_and_two_hours_off_the_hour() {
        let failed = at("2026-10-28T11:14:00Z");
        assert_eq!(retry_at(failed, 0), Some(at("2026-10-28T11:34:00Z")));
        assert_eq!(retry_at(failed, 1), Some(at("2026-10-28T13:14:00Z")));
        assert_eq!(retry_at(failed, 2), None);
        // 10:41 + 20 min is 11:01: moved to 11:07.
        assert_eq!(
            retry_at(at("2026-10-28T10:41:00Z"), 0),
            Some(at("2026-10-28T11:07:00Z"))
        );
    }

    #[test]
    fn a_restart_catches_up_only_past_twenty_hours() {
        let started = at("2026-10-28T15:10:00Z");
        assert_eq!(
            catch_up_at(Some(started - mins(20 * 60 + 1)), started),
            Some(started + mins(3))
        );
        assert_eq!(catch_up_at(None, started), Some(started + mins(3)));
        assert_eq!(catch_up_at(Some(started - mins(20 * 60)), started), None);
        assert_eq!(catch_up_at(Some(started - mins(60)), started), None);
        // A start at 14:58 catches up at 15:07, not 15:01.
        assert_eq!(
            catch_up_at(None, at("2026-10-28T14:58:00Z")),
            Some(at("2026-10-28T15:07:00Z"))
        );
    }
}
