//! Pure logic: money, dates and the rules on them.
//!
//! No axum, no sqlx, no tokio, no I/O: `scripts/layering.py` fails the check if any of them
//! reaches this crate's dependency tree.

pub mod accounts;
pub mod actual;
pub mod categories;
mod currency;
mod date;
pub mod login;
mod money;
pub mod profile;
mod setup_code;
pub mod throttle;
mod timezone;

pub use currency::{CURRENCIES, Currency, UnknownCurrency};
pub use date::{Date, DateError};
pub use money::Money;
pub use setup_code::SetupCode;
pub use timezone::{TimeZone, UnknownTimeZone};
