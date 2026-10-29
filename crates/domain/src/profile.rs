//! A login's own display settings: the locale its dates and numbers are shown in, and the day its
//! weeks start on (docs/spec/foundations.md#conventions). Stored data never depends on them.

use std::fmt;
use std::str::FromStr;

/// A BCP 47 language tag, like `en-US` or `de-DE`. Checked for shape only: the browser knows which
/// locales it can format, and falls back for one it doesn't *(agent)*.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Locale(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Choose a locale from the list.")]
pub struct InvalidLocale;

impl Locale {
    /// The longest tag accepted: room for language, script, region and a variant *(agent)*.
    pub const MAX_LEN: usize = 35;

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Locale {
    type Err = InvalidLocale;

    /// A language subtag of 2–3 or 5–8 letters, then any subtags of 1–8 letters and digits, joined
    /// by `-`.
    fn from_str(tag: &str) -> Result<Self, Self::Err> {
        if tag.len() > Self::MAX_LEN {
            return Err(InvalidLocale);
        }
        let mut subtags = tag.split('-');
        let language = subtags.next().unwrap_or_default();
        let language_ok = matches!(language.len(), 2 | 3 | 5..=8)
            && language.chars().all(|c| c.is_ascii_alphabetic());
        let rest_ok = subtags.all(|subtag| {
            (1..=8).contains(&subtag.len()) && subtag.chars().all(|c| c.is_ascii_alphanumeric())
        });
        if language_ok && rest_ok {
            Ok(Locale(tag.to_owned()))
        } else {
            Err(InvalidLocale)
        }
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The day weeks start on: 0 is Sunday, 6 is Saturday, as the schema stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WeekStart(u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Choose a day of the week.")]
pub struct InvalidWeekStart;

impl WeekStart {
    /// # Errors
    /// If `day` isn't 0 (Sunday) to 6 (Saturday).
    pub fn new(day: i64) -> Result<Self, InvalidWeekStart> {
        u8::try_from(day)
            .ok()
            .filter(|day| *day <= 6)
            .map(WeekStart)
            .ok_or(InvalidWeekStart)
    }

    #[must_use]
    pub fn day(self) -> u8 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_language_tags() {
        for tag in [
            "en",
            "en-US",
            "de-DE",
            "zh-Hans-CN",
            "es-419",
            "sr-Latn-RS",
            "ast",
        ] {
            assert_eq!(tag.parse::<Locale>().unwrap().as_str(), tag);
        }
        assert_eq!("de-DE".parse::<Locale>().unwrap().to_string(), "de-DE");
    }

    #[test]
    fn refuses_what_isnt_a_language_tag() {
        for tag in [
            "",
            "e",
            "languages",
            "en_US",
            "en-",
            "-US",
            "en--US",
            "en-US!",
            "12-US",
            "en-toolongsubtag",
            "en-US; DROP TABLE",
            &format!("en-{}", "x-".repeat(20)),
        ] {
            assert_eq!(tag.parse::<Locale>(), Err(InvalidLocale), "{tag}");
        }
    }

    #[test]
    fn weeks_start_on_one_of_seven_days() {
        assert_eq!(WeekStart::new(0).unwrap().day(), 0);
        assert_eq!(WeekStart::new(6).unwrap().day(), 6);
        assert_eq!(WeekStart::new(7), Err(InvalidWeekStart));
        assert_eq!(WeekStart::new(-1), Err(InvalidWeekStart));
    }

    #[test]
    fn explains_the_errors() {
        assert_eq!(InvalidLocale.to_string(), "Choose a locale from the list.");
        assert_eq!(InvalidWeekStart.to_string(), "Choose a day of the week.");
    }
}
