//! Reads an Actual export zip (a read-only SQLite file) into domain import types
//! (docs/spec/m1-ledger-import.md#reading-the-export).
//!
//! [`unpack`] takes `db.sqlite` and `metadata.json` out of the zip into a folder of the caller's;
//! [`Export::open`] opens that copy read-only and reads Actual's raw tables, never its views.

mod export;
mod unpack;

pub use export::{Export, Summary};
pub use unpack::unpack;

/// The export's database, in the zip and in the folder [`unpack`] writes.
pub const DB_FILE: &str = "db.sqlite";
/// The export's budget-level details (the budget's name), beside [`DB_FILE`].
pub const METADATA_FILE: &str = "metadata.json";

/// Why an export can't be read. The first three are the person's to fix, and say how.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error(
        "This isn't an Actual export. In Actual, use Settings › Export data, and upload the .zip it saves."
    )]
    NotAnExport,
    #[error("This export is too large for the app to read.")]
    TooLarge,
    /// The file is an Actual export, but a value in it isn't one the app understands.
    #[error("This Actual export has data the app can't read: {0}.")]
    Unreadable(String),
    #[error("can't read or write the export's files: {0}")]
    Io(#[from] std::io::Error),
}
