//! Reading the bank feed (docs/spec/m2-bank-data.md#reading-the-feed): a row's amount in minor
//! units, whether it is pending, and its date, the way the live Bridge sends them; and the
//! windows the requests ask for (docs/spec/m2-bank-data.md#the-request).

use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::{Currency, Date, Money};

/// One transaction as the feed sends it, before it is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedRow {
    /// The provider's transaction id (`TRN-…`), opaque.
    pub id: String,
    /// Epoch seconds. Some banks send a future date on pending rows, so it never says pending.
    pub posted: i64,
    /// A decimal string, like `-12.34`.
    pub amount: String,
    /// The raw bank text.
    pub description: String,
    /// The provider's cleaned name, if it sends one.
    pub payee: Option<String>,
    /// Epoch seconds, when the bank says it happened.
    pub transacted_at: Option<i64>,
    /// `Some(true)` only on pending rows; posted rows leave the key out.
    pub pending: Option<bool>,
}

/// A row read from the feed, ready to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncedRow {
    pub external_id: String,
    /// The UTC calendar date of `posted`, or of `transacted_at` while pending.
    pub date: Date,
    pub amount: Money,
    pub pending: bool,
    /// Exactly what the bank sent.
    pub raw_description: String,
    /// What the payee is found or made from: the feed's `payee`, else the description.
    pub payee_text: String,
}

/// Why an account's rows can't be read this run. The account fails for the run and keeps its
/// last data. Never carries an amount or a description.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RowError {
    /// Not a decimal number, or more decimals than the currency has *(agent)*.
    #[error("a row's amount ({id}) isn't an amount in {currency}")]
    Amount { id: String, currency: &'static str },
    /// A date that isn't one.
    #[error("a row's date ({id}) isn't a date")]
    Date { id: String },
}

impl RowError {
    /// The code recorded on the account's result.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            RowError::Amount { .. } => "bad_amount",
            RowError::Date { .. } => "bad_date",
        }
    }
}

/// `text` (`-12.34`, `+5`, `100.5`) in minor units of a currency with `places` decimals. `None`
/// when it isn't a plain decimal number or has more decimals than `places`. No group separators:
/// the feed never sends them.
#[must_use]
pub fn amount(text: &str, places: u8) -> Option<Money> {
    let text = text.trim();
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let digits = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty() || !digits(whole) || !digits(fraction) {
        return None;
    }
    // Trailing zeros past the currency's places (`12.500`) say nothing more *(agent)*.
    let fraction = fraction.trim_end_matches('0');
    let places_used = u32::try_from(fraction.len()).ok()?;
    if places_used > u32::from(places) {
        return None;
    }
    let scale = 10_i64.checked_pow(u32::from(places))?;
    let fraction_minor = if fraction.is_empty() {
        0
    } else {
        fraction
            .parse::<i64>()
            .ok()?
            .checked_mul(10_i64.checked_pow(u32::from(places) - places_used)?)?
    };
    let minor = whole
        .parse::<i64>()
        .ok()?
        .checked_mul(scale)?
        .checked_add(fraction_minor)?;
    Some(Money::from_minor(if negative { -minor } else { minor }))
}

/// The UTC calendar date of `epoch` seconds. Banks encode a date as noon UTC, as midnight
/// Eastern, or as a real clock time; the UTC date is right for all three (and matches what the
/// Actual import stored for the same rows), where the household's timezone would shift some
/// back a day.
#[must_use]
pub fn utc_date(epoch: i64) -> Option<Date> {
    let date = Timestamp::from_second(epoch)
        .ok()?
        .to_zoned(TimeZone::UTC)
        .date();
    Date::from_ymd(date.year(), date.month(), date.day()).ok()
}

/// Reads `rows` for a household in `currency`. Order doesn't matter, so it is kept as sent.
///
/// # Errors
/// [`RowError`] on the first row whose amount or date can't be read: the whole account fails for
/// the run.
pub fn read_rows(rows: &[FeedRow], currency: Currency) -> Result<Vec<SyncedRow>, RowError> {
    rows.iter().map(|row| read_row(row, currency)).collect()
}

fn read_row(row: &FeedRow, currency: Currency) -> Result<SyncedRow, RowError> {
    let amount =
        amount(&row.amount, currency.decimal_places()).ok_or_else(|| RowError::Amount {
            id: row.id.clone(),
            currency: currency.code(),
        })?;
    let pending = row.pending == Some(true);
    // Pending: when it happened. Posted: when it posted. A posted row with no `posted` (0) falls
    // back to `transacted_at` *(agent)*.
    let transacted = row.transacted_at.filter(|at| *at != 0);
    let epoch = if pending {
        transacted.unwrap_or(row.posted)
    } else if row.posted == 0 {
        transacted.unwrap_or(0)
    } else {
        row.posted
    };
    let date = (epoch != 0)
        .then(|| utc_date(epoch))
        .flatten()
        .ok_or_else(|| RowError::Date { id: row.id.clone() })?;
    let payee_text = row
        .payee
        .as_deref()
        .map(str::trim)
        .filter(|payee| !payee.is_empty())
        .unwrap_or_else(|| row.description.trim())
        .to_owned();
    Ok(SyncedRow {
        external_id: row.id.clone(),
        date,
        amount,
        pending,
        raw_description: row.description.clone(),
        payee_text,
    })
}

const DAY: i64 = 86_400;
/// How far back the daily sync, Sync now, retries and catch-ups reach.
pub const DAILY_WINDOW_DAYS: i64 = 30;
/// The most one request ever asks for.
pub const MAX_WINDOW_DAYS: i64 = 45;

/// What one request asks for: rows transacted at or after `start`, and before `end` if set
/// (epoch seconds).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub start: i64,
    pub end: Option<i64>,
}

/// The daily window at `now` (epoch seconds): the last 30 days, no end.
#[must_use]
pub fn daily_window(now: i64) -> Window {
    Window {
        start: now - DAILY_WINDOW_DAYS * DAY,
        end: None,
    }
}

/// The back-fill at `now`: 90 days as two requests of 45, the newest first. The older one ends
/// where the newer starts, so no row comes twice and none is missed *(agent)*.
#[must_use]
pub fn backfill_windows(now: i64) -> [Window; 2] {
    let split = now - MAX_WINDOW_DAYS * DAY;
    [
        Window {
            start: split,
            end: None,
        },
        Window {
            start: split - MAX_WINDOW_DAYS * DAY,
            end: Some(split),
        },
    ]
}

/// A message from the feed as the household may see it *(agent)*: control characters become
/// spaces, whitespace is collapsed, at most 300 characters.
#[must_use]
pub fn sanitize_message(text: &str) -> String {
    let kept: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    kept.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(300)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd() -> Currency {
        "USD".parse().unwrap()
    }

    fn date(text: &str) -> Date {
        let d: jiff::civil::Date = text.parse().unwrap();
        Date::from_ymd(d.year(), d.month(), d.day()).unwrap()
    }

    fn epoch(text: &str) -> i64 {
        text.parse::<Timestamp>().unwrap().as_second()
    }

    fn row(posted: i64, transacted_at: Option<i64>, pending: Option<bool>) -> FeedRow {
        FeedRow {
            id: "TRN-demo-1".to_owned(),
            posted,
            amount: "-12.34".to_owned(),
            description: "DEMO CAFE #4".to_owned(),
            payee: Some("Demo Cafe".to_owned()),
            transacted_at,
            pending,
        }
    }

    fn read(row: &FeedRow) -> SyncedRow {
        read_rows(std::slice::from_ref(row), usd())
            .unwrap()
            .remove(0)
    }

    #[test]
    fn amounts_become_minor_units() {
        let cases = [
            ("-12.34", 2, -1234),
            ("12.3", 2, 1230),
            ("+5", 2, 500),
            ("0.00", 2, 0),
            ("2400.00", 2, 240_000),
            ("-0.5", 2, -50),
            ("12.500", 2, 1250),
            ("1500", 0, 1500),
            ("1.234", 3, 1234),
        ];
        for (text, places, minor) in cases {
            assert_eq!(
                amount(text, places),
                Some(Money::from_minor(minor)),
                "{text}"
            );
        }
    }

    #[test]
    fn what_isnt_an_amount_or_has_too_many_decimals_is_refused() {
        for (text, places) in [
            ("", 2),
            ("-", 2),
            ("abc", 2),
            ("1,234.00", 2),
            (".50", 2),
            ("12.345", 2),
            ("12.5", 0),
            ("1e3", 2),
            ("99999999999999999999", 2),
        ] {
            assert_eq!(amount(text, places), None, "{text}");
        }
    }

    #[test]
    fn too_many_decimals_fails_the_account_without_saying_the_amount() {
        let mut bad = row(epoch("2026-10-20T12:00:00Z"), None, None);
        bad.amount = "-12.345".to_owned();
        let error = read_rows(&[row(1, Some(1), None), bad], usd()).unwrap_err();
        assert_eq!(error.code(), "bad_amount");
        assert!(!error.to_string().contains("12.345"));
        let mut undated = row(0, None, None);
        undated.id = "TRN-x".to_owned();
        assert_eq!(
            read_rows(std::slice::from_ref(&undated), usd())
                .unwrap_err()
                .code(),
            "bad_date"
        );
        assert_eq!(
            read_rows(&[undated], usd()).unwrap_err(),
            RowError::Date {
                id: "TRN-x".to_owned()
            }
        );
    }

    #[test]
    fn a_date_at_noon_utc_is_that_date() {
        let read = read(&row(epoch("2026-10-20T12:00:00Z"), None, None));
        assert_eq!(read.date, date("2026-10-20"));
    }

    #[test]
    fn local_midnight_eastern_is_the_utc_date_in_and_out_of_daylight_time() {
        // Daylight time: midnight EDT is 04:00 UTC the same day.
        let summer = read(&row(epoch("2026-07-15T04:00:00Z"), None, None));
        assert_eq!(summer.date, date("2026-07-15"));
        // Standard time: midnight EST is 05:00 UTC the same day.
        let winter = read(&row(epoch("2026-12-15T05:00:00Z"), None, None));
        assert_eq!(winter.date, date("2026-12-15"));
    }

    #[test]
    fn clock_times_take_the_utc_date() {
        let morning = read(&row(epoch("2026-10-20T00:30:00Z"), None, None));
        assert_eq!(morning.date, date("2026-10-20"));
        let night = read(&row(epoch("2026-10-20T23:59:59Z"), None, None));
        assert_eq!(night.date, date("2026-10-20"));
    }

    #[test]
    fn posted_rows_without_the_pending_key_are_posted_and_dated_by_posted() {
        let posted = read(&row(
            epoch("2026-10-22T12:00:00Z"),
            Some(epoch("2026-10-20T18:00:00Z")),
            None,
        ));
        assert!(!posted.pending);
        assert_eq!(posted.date, date("2026-10-22"));
        assert_eq!(posted.amount, Money::from_minor(-1234));
        assert_eq!(posted.raw_description, "DEMO CAFE #4");
        assert_eq!(posted.payee_text, "Demo Cafe");
        assert!(!read(&row(epoch("2026-10-22T12:00:00Z"), None, Some(false))).pending);
    }

    #[test]
    fn a_pending_row_with_a_posted_date_is_pending_and_dated_by_transacted_at() {
        // Some banks send a future `posted` on pending rows.
        let pending = read(&row(
            epoch("2026-10-30T12:00:00Z"),
            Some(epoch("2026-10-27T15:00:00Z")),
            Some(true),
        ));
        assert!(pending.pending);
        assert_eq!(pending.date, date("2026-10-27"));
        // Without `transacted_at`, `posted` is all there is.
        let bare = read(&row(epoch("2026-10-27T12:00:00Z"), None, Some(true)));
        assert_eq!(bare.date, date("2026-10-27"));
        // `posted == 0` never makes a row pending by itself.
        let zero = read(&row(0, Some(epoch("2026-10-26T12:00:00Z")), None));
        assert!(!zero.pending);
        assert_eq!(zero.date, date("2026-10-26"));
    }

    #[test]
    fn the_payee_falls_back_to_the_description() {
        let mut bare = row(epoch("2026-10-20T12:00:00Z"), None, None);
        bare.payee = Some("  ".to_owned());
        assert_eq!(read(&bare).payee_text, "DEMO CAFE #4");
        bare.payee = None;
        assert_eq!(read(&bare).payee_text, "DEMO CAFE #4");
    }

    #[test]
    fn order_is_kept_as_sent() {
        let mut later = row(epoch("2026-10-22T12:00:00Z"), None, None);
        later.id = "TRN-b".to_owned();
        let earlier = row(epoch("2026-10-20T12:00:00Z"), None, None);
        let read = read_rows(&[later, earlier], usd()).unwrap();
        assert_eq!(read[0].external_id, "TRN-b");
    }

    #[test]
    fn the_windows_never_ask_for_more_than_45_days() {
        let now = epoch("2026-10-28T09:00:00Z");
        assert_eq!(
            daily_window(now),
            Window {
                start: now - 30 * DAY,
                end: None
            }
        );
        let [newer, older] = backfill_windows(now);
        assert_eq!(newer.start, now - 45 * DAY);
        assert_eq!(newer.end, None);
        assert_eq!(older.start, now - 90 * DAY);
        assert_eq!(older.end, Some(newer.start));
    }

    #[test]
    fn messages_are_sanitized() {
        assert_eq!(
            sanitize_message(" Demo\u{7}Bank\nneeds  you "),
            "Demo Bank needs you"
        );
        assert_eq!(sanitize_message(&"x".repeat(400)).len(), 300);
    }
}
