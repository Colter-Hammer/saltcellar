use std::fmt;

/// The one-time code that claims a fresh install: eight characters, printed to the log at startup
/// while setup isn't finished, and asked for first by the setup wizard (docs/spec/foundations.md#auth).
///
/// The alphabet leaves out `0`, `1`, `I` and `O`, which read alike. Shown as `ABCD-EFGH`; typed
/// any way, since case, spaces and dashes are ignored.
#[derive(Clone, PartialEq, Eq)]
pub struct SetupCode([u8; SetupCode::LEN]);

/// 32 symbols, so each random byte maps to one without bias.
const ALPHABET: &[u8; 32] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";

impl SetupCode {
    pub const LEN: usize = 8;

    /// A code from random bytes; the caller supplies them from the OS.
    #[must_use]
    pub fn from_random(bytes: [u8; Self::LEN]) -> Self {
        SetupCode(bytes.map(|b| ALPHABET[usize::from(b & 31)]))
    }

    /// Whether `typed` is this code. Every character is compared, whatever the first mismatch.
    #[must_use]
    pub fn matches(&self, typed: &str) -> bool {
        let typed: Vec<u8> = typed
            .bytes()
            .filter(|b| !matches!(b, b'-' | b' ' | b'\t'))
            .map(|b| b.to_ascii_uppercase())
            .collect();
        typed.len() == Self::LEN
            && typed
                .iter()
                .zip(self.0)
                .fold(0, |diff, (a, b)| diff | (a ^ b))
                == 0
    }
}

impl fmt::Display for SetupCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (first, second) = self.0.split_at(Self::LEN / 2);
        for b in first {
            write!(f, "{}", char::from(*b))?;
        }
        f.write_str("-")?;
        for b in second {
            write!(f, "{}", char::from(*b))?;
        }
        Ok(())
    }
}

/// Hidden, so the code only reaches the log where it's printed on purpose.
impl fmt::Debug for SetupCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SetupCode(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code() -> SetupCode {
        SetupCode::from_random([0, 1, 8, 9, 31, 32, 200, 255])
    }

    #[test]
    fn maps_bytes_onto_the_alphabet_in_two_groups() {
        assert_eq!(code().to_string(), "23AB-Z2AZ");
    }

    #[test]
    fn leaves_out_characters_that_read_alike() {
        let every = SetupCode::from_random([0; 8]).to_string();
        assert_eq!(every, "2222-2222");
        for c in ['0', '1', 'I', 'O'] {
            assert!(!ALPHABET.contains(&(c as u8)), "{c}");
        }
    }

    #[test]
    fn matches_however_it_is_typed() {
        for typed in ["23AB-Z2AZ", "23abz2az", " 23ab z2az ", "23ab-z2az\t"] {
            assert!(code().matches(typed), "{typed:?}");
        }
    }

    #[test]
    fn rejects_other_codes() {
        for typed in [
            "",
            "23AB-Z2AY",
            "23AB-Z2A",
            "23AB-Z2AZZ",
            "Z2AZ-23AB",
            "23AB_Z2AZ",
        ] {
            assert!(!code().matches(typed), "{typed:?}");
        }
    }

    #[test]
    fn never_shows_in_debug_output() {
        assert_eq!(format!("{:?}", code()), "SetupCode(..)");
    }
}
