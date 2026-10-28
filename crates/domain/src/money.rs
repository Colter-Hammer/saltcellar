use std::fmt;

/// An amount in integer minor units of the household currency (cents for USD, yen for JPY).
///
/// Never a float. The sign is from the account's point of view: money leaving is negative.
/// How many minor units make a major one comes from the currency, which `Money` doesn't carry:
/// formatting for display happens in the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Money(i64);

impl Money {
    pub const ZERO: Money = Money(0);

    #[must_use]
    pub const fn from_minor(minor: i64) -> Self {
        Money(minor)
    }

    #[must_use]
    pub const fn minor(self) -> i64 {
        self.0
    }

    #[must_use]
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// `None` on overflow, which real amounts never reach; callers turn it into an error.
    #[must_use]
    pub const fn checked_add(self, other: Money) -> Option<Money> {
        match self.0.checked_add(other.0) {
            Some(sum) => Some(Money(sum)),
            None => None,
        }
    }

    /// `None` on overflow.
    #[must_use]
    pub const fn checked_sub(self, other: Money) -> Option<Money> {
        match self.0.checked_sub(other.0) {
            Some(difference) => Some(Money(difference)),
            None => None,
        }
    }

    /// The same amount the other way round. `None` only for `i64::MIN`.
    #[must_use]
    pub const fn checked_neg(self) -> Option<Money> {
        match self.0.checked_neg() {
            Some(negated) => Some(Money(negated)),
            None => None,
        }
    }

    /// The total of `amounts`, or `None` if it overflows.
    pub fn checked_sum(amounts: impl IntoIterator<Item = Money>) -> Option<Money> {
        amounts
            .into_iter()
            .try_fold(Money::ZERO, Money::checked_add)
    }
}

/// Minor units, e.g. `-1250`. Only for logs and tests: users see amounts formatted by the frontend.
impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_minor_units_exactly() {
        assert_eq!(Money::from_minor(-1250).minor(), -1250);
        assert_eq!(Money::default(), Money::ZERO);
    }

    #[test]
    fn sign_and_zero() {
        assert!(Money::from_minor(-1).is_negative());
        assert!(!Money::from_minor(1).is_negative());
        assert!(!Money::ZERO.is_negative());
        assert!(Money::ZERO.is_zero());
        assert!(!Money::from_minor(5).is_zero());
    }

    #[test]
    fn adds_and_subtracts() {
        let a = Money::from_minor(1000);
        let b = Money::from_minor(-250);
        assert_eq!(a.checked_add(b), Some(Money::from_minor(750)));
        assert_eq!(a.checked_sub(b), Some(Money::from_minor(1250)));
    }

    #[test]
    fn overflow_is_none_not_a_wrap() {
        let max = Money::from_minor(i64::MAX);
        let min = Money::from_minor(i64::MIN);
        assert_eq!(max.checked_add(Money::from_minor(1)), None);
        assert_eq!(min.checked_sub(Money::from_minor(1)), None);
        assert_eq!(min.checked_neg(), None);
    }

    #[test]
    fn negates() {
        assert_eq!(
            Money::from_minor(399).checked_neg(),
            Some(Money::from_minor(-399))
        );
    }

    #[test]
    fn sums() {
        let amounts = [
            Money::from_minor(100),
            Money::from_minor(-30),
            Money::from_minor(5),
        ];
        assert_eq!(Money::checked_sum(amounts), Some(Money::from_minor(75)));
        assert_eq!(Money::checked_sum([]), Some(Money::ZERO));
        assert_eq!(
            Money::checked_sum([Money::from_minor(i64::MAX), Money::from_minor(1)]),
            None
        );
    }

    #[test]
    fn orders_by_amount() {
        assert!(Money::from_minor(-5) < Money::ZERO);
        assert!(Money::from_minor(10) > Money::from_minor(9));
    }

    #[test]
    fn displays_minor_units() {
        assert_eq!(Money::from_minor(-1250).to_string(), "-1250");
    }
}
