//! Pure logic: money, dates and the rules on them.
//!
//! No axum, no sqlx, no tokio, no I/O: `scripts/layering.py` fails the check if any of them
//! reaches this crate's dependency tree.

mod date;
mod money;

pub use date::{Date, DateError};
pub use money::Money;
