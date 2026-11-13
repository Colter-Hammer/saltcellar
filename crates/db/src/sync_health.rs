//! What the last syncs said about each tracked feed account (docs/spec/m2-bank-data.md#errors and
//! #sync-runs): the facts `domain::sync_health` reads its health from.

use domain::sync_health::{Facts, Latest};

use crate::{Error, SqlitePool, parse_stamp};

/// A tracked feed account and what is known about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedAccountHealth {
    pub account_id: i64,
    /// Its name in the app.
    pub name: String,
    /// Its bank link's id (`conn_id`), if the feed names it.
    pub bank_link_id: Option<String>,
    /// Its bank's name, as the feed gives it.
    pub bank_name: Option<String>,
    /// Only its balance is synced.
    pub balances_only: bool,
    pub facts: Facts,
    /// The newest result's sanitized message, if it failed.
    pub message: Option<String>,
}

/// Every tracked feed account of the household's connection with what its newest result says, in
/// the order first seen. The refresh date is the newest one any result had, a failed read's
/// included, since the bank's own date is what shows how old its data is.
///
/// # Errors
/// If the database fails or holds a time it can't read.
pub async fn feed_account_health(pool: &SqlitePool) -> Result<Vec<FeedAccountHealth>, Error> {
    let rows = sqlx::query!(
        r#"SELECT a.id AS "account_id!: i64", a.name, a.has_transactions AS "has_transactions!: bool",
             ca.bank_link_id, ca.institution, ca.last_seen_at,
             (SELECT r.ok FROM sync_account_results r WHERE r.account_id = a.id
               ORDER BY r.sync_run_id DESC LIMIT 1) AS "latest_ok: bool",
             (SELECT r.error_code FROM sync_account_results r WHERE r.account_id = a.id
               ORDER BY r.sync_run_id DESC LIMIT 1) AS latest_code,
             (SELECT r.error FROM sync_account_results r WHERE r.account_id = a.id
               ORDER BY r.sync_run_id DESC LIMIT 1) AS latest_error,
             (SELECT r.provider_balance_date FROM sync_account_results r
               WHERE r.account_id = a.id AND r.provider_balance_date IS NOT NULL
               ORDER BY r.sync_run_id DESC LIMIT 1) AS balance_date
           FROM connection_accounts ca JOIN accounts a ON a.id = ca.account_id
           WHERE ca.decision = 'tracked'
           ORDER BY ca.first_seen_at, ca.rowid"#
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            let ok = row.latest_ok;
            let failed = ok == Some(false);
            Ok(FeedAccountHealth {
                account_id: row.account_id,
                name: row.name,
                bank_link_id: row.bank_link_id,
                bank_name: row.institution,
                balances_only: !row.has_transactions,
                facts: Facts {
                    balance_date: row.balance_date.map(parse_stamp).transpose()?,
                    latest: ok.map(|ok| Latest {
                        ok,
                        error_code: row.latest_code,
                    }),
                    last_seen: Some(parse_stamp(row.last_seen_at)?),
                },
                message: if failed { row.latest_error } else { None },
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "sync_health_tests.rs"]
mod tests;
