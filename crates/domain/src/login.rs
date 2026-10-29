//! The rules on a login's username, display name and password (docs/spec/foundations.md#auth).

use std::fmt;

use jiff::SignedDuration;

/// How long a session lasts since it was last used: 30 days, sliding.
pub const SESSION_LIFETIME: SignedDuration = SignedDuration::from_hours(30 * 24);

/// How long a password reset link works for, once *(agent)*.
pub const RESET_LIFETIME: SignedDuration = SignedDuration::from_hours(24);

/// What a login types to log in: letters, digits, dots, dashes and underscores, kept in lower case
/// so that `Sam` and `sam` are the same login *(agent)*.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Username(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UsernameError {
    #[error("Choose a username.")]
    Empty,
    #[error("Use at most {} characters.", Username::MAX_CHARS)]
    TooLong,
    #[error("Use only letters, digits, dots, dashes and underscores.")]
    Characters,
}

impl Username {
    pub const MAX_CHARS: usize = 64;

    /// The username as typed, trimmed and in lower case.
    ///
    /// # Errors
    /// If it's empty, too long or has a character outside the allowed ones.
    pub fn new(typed: &str) -> Result<Self, UsernameError> {
        let name = typed.trim().to_lowercase();
        if name.is_empty() {
            return Err(UsernameError::Empty);
        }
        if name.chars().count() > Self::MAX_CHARS {
            return Err(UsernameError::TooLong);
        }
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))
        {
            return Err(UsernameError::Characters);
        }
        Ok(Username(name))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Username {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What the app calls a login: any text, trimmed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DisplayName(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DisplayNameError {
    #[error("Enter a name.")]
    Empty,
    #[error("Use at most {} characters.", DisplayName::MAX_CHARS)]
    TooLong,
}

impl DisplayName {
    pub const MAX_CHARS: usize = 100;

    /// # Errors
    /// If it's empty once trimmed, or too long.
    pub fn new(typed: &str) -> Result<Self, DisplayNameError> {
        let name = typed.trim();
        if name.is_empty() {
            return Err(DisplayNameError::Empty);
        }
        if name.chars().count() > Self::MAX_CHARS {
            return Err(DisplayNameError::TooLong);
        }
        Ok(DisplayName(name.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A password that meets the rules: at least 12 characters, no composition rules. Kept exactly as
/// typed, spaces included. Its `Debug` hides it, so it can't reach a log by accident.
#[derive(Clone, PartialEq, Eq)]
pub struct Password(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PasswordError {
    #[error("Use at least {} characters.", Password::MIN_CHARS)]
    TooShort,
    /// A ceiling well past any passphrase, so hashing one can't be made to take long *(agent)*.
    #[error("Use at most {} characters.", Password::MAX_CHARS)]
    TooLong,
    #[error("The passwords don't match.")]
    Mismatch,
}

impl Password {
    pub const MIN_CHARS: usize = 12;
    pub const MAX_CHARS: usize = 1024;

    /// A new password, typed twice.
    ///
    /// # Errors
    /// [`PasswordError::TooShort`] or [`PasswordError::TooLong`] for the first, which win over
    /// [`PasswordError::Mismatch`] when the second doesn't match it.
    pub fn new(typed: &str, again: &str) -> Result<Self, PasswordError> {
        let chars = typed.chars().count();
        if chars < Self::MIN_CHARS {
            return Err(PasswordError::TooShort);
        }
        if chars > Self::MAX_CHARS {
            return Err(PasswordError::TooLong);
        }
        if typed != again {
            return Err(PasswordError::Mismatch);
        }
        Ok(Password(typed.to_owned()))
    }

    /// The password itself, only for hashing.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usernames_are_trimmed_and_lower_case() {
        assert_eq!(Username::new("  Sam.Lee_2 ").unwrap().as_str(), "sam.lee_2");
        assert_eq!(Username::new("Zoë-K").unwrap().to_string(), "zoë-k");
    }

    #[test]
    fn usernames_need_something_allowed() {
        assert_eq!(Username::new("   "), Err(UsernameError::Empty));
        assert_eq!(Username::new("sam lee"), Err(UsernameError::Characters));
        assert_eq!(Username::new("sam@home"), Err(UsernameError::Characters));
        assert_eq!(Username::new(&"a".repeat(64)).unwrap().as_str().len(), 64);
        assert_eq!(Username::new(&"a".repeat(65)), Err(UsernameError::TooLong));
    }

    #[test]
    fn display_names_are_trimmed_and_otherwise_free() {
        assert_eq!(
            DisplayName::new(" Sam O'Neil ").unwrap().as_str(),
            "Sam O'Neil"
        );
        assert_eq!(DisplayName::new(" \t"), Err(DisplayNameError::Empty));
        assert!(DisplayName::new(&"é".repeat(100)).is_ok());
        assert_eq!(
            DisplayName::new(&"é".repeat(101)),
            Err(DisplayNameError::TooLong)
        );
    }

    #[test]
    fn passwords_need_twelve_characters() {
        assert_eq!(
            Password::new("elevenchars", "elevenchars"),
            Err(PasswordError::TooShort)
        );
        let twelve = "twelve chars";
        assert_eq!(Password::new(twelve, twelve).unwrap().expose(), twelve);
    }

    #[test]
    fn password_length_counts_characters_not_bytes() {
        let accents = "éééééééééééé";
        assert_eq!(accents.chars().count(), 12);
        assert!(Password::new(accents, accents).is_ok());
        assert_eq!(
            Password::new(&accents[2..], &accents[2..]),
            Err(PasswordError::TooShort)
        );
    }

    #[test]
    fn passwords_have_a_ceiling() {
        let longest = "x".repeat(1024);
        assert!(Password::new(&longest, &longest).is_ok());
        let longer = "x".repeat(1025);
        assert_eq!(Password::new(&longer, &longer), Err(PasswordError::TooLong));
    }

    #[test]
    fn passwords_must_be_typed_the_same_twice() {
        assert_eq!(
            Password::new("correct horse battery", "correct horse batter"),
            Err(PasswordError::Mismatch)
        );
        assert_eq!(
            Password::new("short", "different"),
            Err(PasswordError::TooShort)
        );
    }

    #[test]
    fn passwords_are_kept_as_typed() {
        let spaced = "  spaces count  ";
        assert_eq!(Password::new(spaced, spaced).unwrap().expose(), spaced);
    }

    #[test]
    fn a_password_never_shows_in_debug_output() {
        let password = Password::new("hunter2hunter2", "hunter2hunter2").unwrap();
        assert_eq!(format!("{password:?}"), "Password(..)");
    }

    #[test]
    fn explains_the_errors() {
        assert_eq!(
            UsernameError::TooLong.to_string(),
            "Use at most 64 characters."
        );
        assert_eq!(
            DisplayNameError::TooLong.to_string(),
            "Use at most 100 characters."
        );
        assert_eq!(
            PasswordError::TooShort.to_string(),
            "Use at least 12 characters."
        );
        assert_eq!(
            PasswordError::TooLong.to_string(),
            "Use at most 1024 characters."
        );
    }
}
