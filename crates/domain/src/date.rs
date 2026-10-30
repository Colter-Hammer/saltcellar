use std::fmt;
use std::str::FromStr;

/// A plain calendar date, `YYYY-MM-DD`, with no time and no timezone.
///
/// "Today" is a date in the household timezone; turning an instant into a `Date` happens at the
/// edges, never here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date(jiff::civil::Date);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DateError {
    #[error("expected a date as YYYY-MM-DD, got {0:?}")]
    Format(String),
    #[error("{0} is not a day on the calendar")]
    NoSuchDay(String),
}

impl Date {
    /// The date `year`-`month`-`day`, if it's on the calendar (years 1 to 9999).
    ///
    /// # Errors
    /// [`DateError::NoSuchDay`] for e.g. February 30th or month 13.
    pub fn from_ymd(year: i16, month: i8, day: i8) -> Result<Self, DateError> {
        let no_such_day = || DateError::NoSuchDay(format!("{year:04}-{month:02}-{day:02}"));
        let date = jiff::civil::Date::new(year, month, day).map_err(|_| no_such_day())?;
        if date.year() < 1 {
            return Err(no_such_day());
        }
        Ok(Date(date))
    }

    #[must_use]
    pub fn year(self) -> i16 {
        self.0.year()
    }

    #[must_use]
    pub fn month(self) -> i8 {
        self.0.month()
    }

    #[must_use]
    pub fn day(self) -> i8 {
        self.0.day()
    }

    /// The day before; `None` before the first day there is (0001-01-01).
    #[must_use]
    pub fn previous_day(self) -> Option<Date> {
        let day = self.0.yesterday().ok()?;
        (day.year() >= 1).then_some(Date(day))
    }
}

/// Exactly `YYYY-MM-DD`: four-digit year, two-digit month and day, nothing before or after.
impl FromStr for Date {
    type Err = DateError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = s.as_bytes();
        let shape_ok = bytes.len() == 10
            && bytes.iter().enumerate().all(|(i, b)| match i {
                4 | 7 => *b == b'-',
                _ => b.is_ascii_digit(),
            });
        if !shape_ok {
            return Err(DateError::Format(s.to_owned()));
        }
        let date: jiff::civil::Date = s.parse().map_err(|_| DateError::NoSuchDay(s.to_owned()))?;
        Date::from_ymd(date.year(), date.month(), date.day())
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02}",
            self.year(),
            self.month(),
            self.day()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(s: &str) -> Date {
        s.parse().unwrap()
    }

    #[test]
    fn the_day_before() {
        assert_eq!(date("2026-03-01").previous_day(), Some(date("2026-02-28")));
        assert_eq!(date("2025-01-01").previous_day(), Some(date("2024-12-31")));
        assert_eq!(date("0001-01-01").previous_day(), None);
    }

    #[test]
    fn parses_and_prints_the_same_text() {
        for s in ["2026-10-28", "0001-01-01", "9999-12-31", "2024-02-29"] {
            assert_eq!(date(s).to_string(), s);
        }
    }

    #[test]
    fn exposes_its_parts() {
        let d = date("2026-03-07");
        assert_eq!((d.year(), d.month(), d.day()), (2026, 3, 7));
    }

    #[test]
    fn builds_from_parts() {
        assert_eq!(Date::from_ymd(2026, 3, 7), Ok(date("2026-03-07")));
        assert_eq!(
            Date::from_ymd(2026, 2, 30),
            Err(DateError::NoSuchDay("2026-02-30".to_owned()))
        );
        assert_eq!(
            Date::from_ymd(0, 1, 1),
            Err(DateError::NoSuchDay("0000-01-01".to_owned()))
        );
    }

    #[test]
    fn rejects_days_not_on_the_calendar() {
        for s in [
            "2026-02-29",
            "2026-13-01",
            "2026-00-10",
            "2026-04-31",
            "0000-01-01",
        ] {
            assert_eq!(
                s.parse::<Date>(),
                Err(DateError::NoSuchDay(s.to_owned())),
                "{s}"
            );
        }
    }

    #[test]
    fn rejects_anything_but_yyyy_mm_dd() {
        for s in [
            "",
            "2026-1-01",
            "2026/10/28",
            "26-10-28",
            "2026-10-28T00:00",
            " 2026-10-28",
            "+026-10-28",
            "2026-1a-28",
            "２０２６-10-28",
        ] {
            assert_eq!(
                s.parse::<Date>(),
                Err(DateError::Format(s.to_owned())),
                "{s:?}"
            );
        }
    }

    #[test]
    fn orders_by_calendar() {
        assert!(date("2025-12-31") < date("2026-01-01"));
        assert!(date("2026-02-09") < date("2026-02-10"));
    }

    #[test]
    fn explains_errors() {
        assert_eq!(
            "nope".parse::<Date>().unwrap_err().to_string(),
            "expected a date as YYYY-MM-DD, got \"nope\""
        );
        assert_eq!(
            "2026-02-30".parse::<Date>().unwrap_err().to_string(),
            "2026-02-30 is not a day on the calendar"
        );
    }
}
