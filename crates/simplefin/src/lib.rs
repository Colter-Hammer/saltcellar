//! SimpleFIN Bridge (docs/spec/m2-bank-data.md#the-connection): the secret key, the access URL
//! sealed with it, claiming a setup token, and reading the feed.
//!
//! The access URL is a long-lived credential. It lives only in an [`AccessUrl`], whose `Debug`
//! and `Display` print a placeholder, and in the database sealed under the [`SecretKey`]. No error
//! in this crate carries it, or the setup token it came from.

mod claim;
mod feed;
mod secret;

pub use claim::{AccessUrl, ClaimError, ClaimPolicy, ClaimUrl, TokenError, claim};
pub use feed::{Feed, FeedAccount, FeedProblem, FetchError, fetch, fetch_accounts, fetch_with_body};
pub use secret::{
    KeyError, OpenError, SealError, SecretKey, open, open_raw_response, seal, seal_raw_response,
};
