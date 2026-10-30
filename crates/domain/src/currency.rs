use std::fmt;
use std::str::FromStr;

/// The household currency: an ISO 4217 code in current use, e.g. `USD`.
///
/// One per install, chosen at setup ([ADR-0006](../../../docs/adr/0006-one-currency-one-timezone.md)).
/// The frontend formats amounts through `Intl.NumberFormat`; [`Currency::decimal_places`] agrees
/// with it, so the server can tell which currencies an import in cents fits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Currency(&'static str);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Choose a currency from the list.")]
pub struct UnknownCurrency;

/// Every ISO 4217 currency in circulation, sorted: no funds codes, metals or withdrawn currencies.
pub const CURRENCIES: &[&str] = &[
    "AED", "AFN", "ALL", "AMD", "AOA", "ARS", "AUD", "AWG", "AZN", "BAM", "BBD", "BDT", "BHD",
    "BIF", "BMD", "BND", "BOB", "BRL", "BSD", "BTN", "BWP", "BYN", "BZD", "CAD", "CDF", "CHF",
    "CLP", "CNY", "COP", "CRC", "CUP", "CVE", "CZK", "DJF", "DKK", "DOP", "DZD", "EGP", "ERN",
    "ETB", "EUR", "FJD", "FKP", "GBP", "GEL", "GHS", "GIP", "GMD", "GNF", "GTQ", "GYD", "HKD",
    "HNL", "HTG", "HUF", "IDR", "ILS", "INR", "IQD", "IRR", "ISK", "JMD", "JOD", "JPY", "KES",
    "KGS", "KHR", "KMF", "KPW", "KRW", "KWD", "KYD", "KZT", "LAK", "LBP", "LKR", "LRD", "LSL",
    "LYD", "MAD", "MDL", "MGA", "MKD", "MMK", "MNT", "MOP", "MRU", "MUR", "MVR", "MWK", "MXN",
    "MYR", "MZN", "NAD", "NGN", "NIO", "NOK", "NPR", "NZD", "OMR", "PAB", "PEN", "PGK", "PHP",
    "PKR", "PLN", "PYG", "QAR", "RON", "RSD", "RUB", "RWF", "SAR", "SBD", "SCR", "SDG", "SEK",
    "SGD", "SHP", "SLE", "SOS", "SRD", "SSP", "STN", "SVC", "SYP", "SZL", "THB", "TJS", "TMT",
    "TND", "TOP", "TRY", "TTD", "TWD", "TZS", "UAH", "UGX", "USD", "UYU", "UZS", "VES", "VND",
    "VUV", "WST", "XAF", "XCD", "XCG", "XOF", "XPF", "YER", "ZAR", "ZMW", "ZWG",
];

/// The listed currencies without 2 decimal places, as `Intl.NumberFormat` (CLDR) formats them:
/// these have none.
const NO_DECIMALS: &[&str] = &[
    "AFN", "ALL", "BIF", "CLP", "COP", "DJF", "GNF", "HUF", "IDR", "IQD", "IRR", "ISK", "JPY",
    "KMF", "KPW", "KRW", "LAK", "LBP", "MGA", "MMK", "PKR", "PYG", "RWF", "SOS", "SYP", "UGX",
    "VND", "VUV", "XAF", "XOF", "XPF", "YER",
];

/// And these have 3.
const THREE_DECIMALS: &[&str] = &["BHD", "JOD", "KWD", "LYD", "OMR", "TND"];

impl Currency {
    /// The ISO 4217 code, e.g. `USD`.
    #[must_use]
    pub const fn code(self) -> &'static str {
        self.0
    }

    /// How many minor units make a major one, as a power of ten: 2 for USD (cents), 0 for JPY.
    #[must_use]
    pub fn decimal_places(self) -> u8 {
        if NO_DECIMALS.binary_search(&self.0).is_ok() {
            0
        } else if THREE_DECIMALS.binary_search(&self.0).is_ok() {
            3
        } else {
            2
        }
    }
}

/// Exactly one of [`CURRENCIES`], in capitals.
impl FromStr for Currency {
    type Err = UnknownCurrency;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        CURRENCIES
            .binary_search(&s)
            .ok()
            .and_then(|i| CURRENCIES.get(i))
            .map(|code| Currency(code))
            .ok_or(UnknownCurrency)
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_sorted_and_every_code_is_three_capitals() {
        assert!(CURRENCIES.is_sorted_by(|a, b| a < b));
        for code in CURRENCIES {
            assert!(
                code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase()),
                "{code}"
            );
        }
    }

    #[test]
    fn parses_every_listed_code() {
        for code in CURRENCIES {
            assert_eq!(code.parse::<Currency>().unwrap().code(), *code);
        }
        assert_eq!("JPY".parse::<Currency>().unwrap().to_string(), "JPY");
    }

    #[test]
    fn knows_each_currencys_decimal_places() {
        for list in [NO_DECIMALS, THREE_DECIMALS] {
            assert!(list.is_sorted_by(|a, b| a < b));
            assert!(
                list.iter().all(|code| CURRENCIES.contains(code)),
                "{list:?}"
            );
        }
        let places = |code: &str| code.parse::<Currency>().unwrap().decimal_places();
        assert_eq!(places("USD"), 2);
        assert_eq!(places("EUR"), 2);
        assert_eq!(places("JPY"), 0);
        assert_eq!(places("KWD"), 3);
    }

    #[test]
    fn rejects_anything_else() {
        for s in ["", "usd", "US", "USDD", "XAU", "HRK", "ABC", " USD"] {
            assert_eq!(s.parse::<Currency>(), Err(UnknownCurrency), "{s:?}");
        }
    }

    #[test]
    fn explains_the_error() {
        assert_eq!(
            UnknownCurrency.to_string(),
            "Choose a currency from the list."
        );
    }
}
