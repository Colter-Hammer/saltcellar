use std::fmt;
use std::str::FromStr;

/// The household timezone: an IANA name, e.g. `America/Chicago`. It decides what "today" is.
///
/// Checked against the timezone database bundled into the binary, so it doesn't depend on the host.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TimeZone(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Choose a timezone from the list.")]
pub struct UnknownTimeZone;

impl TimeZone {
    /// The IANA name, in the database's own spelling.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.0
    }
}

/// A name in the IANA database, matched without regard to case and stored in its canonical case.
impl FromStr for TimeZone {
    type Err = UnknownTimeZone;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let zone = jiff::tz::db().get(s).map_err(|_| UnknownTimeZone)?;
        let name = zone.iana_name().ok_or(UnknownTimeZone)?;
        Ok(TimeZone(name.to_owned()))
    }
}

impl fmt::Display for TimeZone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iana_names() {
        for name in ["America/Chicago", "Europe/Berlin", "Asia/Kolkata", "UTC"] {
            assert_eq!(name.parse::<TimeZone>().unwrap().name(), name);
        }
    }

    #[test]
    fn keeps_the_canonical_spelling() {
        let zone: TimeZone = "america/new_york".parse().unwrap();
        assert_eq!(zone.to_string(), "America/New_York");
    }

    #[test]
    fn rejects_anything_else() {
        for s in [
            "",
            "Mars/Olympus_Mons",
            "EST5EDT6",
            "+02:00",
            "/etc/localtime",
        ] {
            assert_eq!(s.parse::<TimeZone>(), Err(UnknownTimeZone), "{s:?}");
        }
    }

    #[test]
    fn explains_the_error() {
        assert_eq!(
            UnknownTimeZone.to_string(),
            "Choose a timezone from the list."
        );
    }
}
