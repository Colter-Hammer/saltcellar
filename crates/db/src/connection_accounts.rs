//! The new accounts prompt (docs/spec/m2-bank-data.md#accounts): every account the bank
//! connection reports is recorded in `connection_accounts`; one not linked yet waits until the
//! household tracks it (creating an account, or moving an existing one onto it with "This is the
//! same as…") or ignores it.

use domain::accounts::{AccountType, Role};
use jiff::Timestamp;

use crate::{Error, SqlitePool, stamp};

/// An account as the feed reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportedAccount {
    /// The provider's account id.
    pub external_id: String,
    pub name: String,
    pub institution: Option<String>,
    /// ISO 4217, as the feed spells it.
    pub currency: String,
}

/// What recording a response did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recorded {
    /// Accounts seen for the first time.
    pub new: u64,
    /// Accounts the Actual import made with the same provider id, attached as tracked at once.
    pub attached: u64,
    /// The names of the accounts in another currency than the household's, never tracked.
    pub refused: Vec<String>,
}

/// Records every account in one response from the household's connection `connection_id`, seen
/// at `now`: new ones wait in the prompt, known ones get their name and bank refreshed. A waiting
/// one whose provider id is on an account with no other feed account (the Actual import's, or one
/// a disconnect left) is attached to it as tracked, skipping the prompt. One in another currency
/// than `household_currency` is recorded and never attached.
///
/// # Errors
/// If the database fails.
pub async fn record_connection_accounts(
    pool: &SqlitePool,
    connection_id: i64,
    reported: &[ReportedAccount],
    household_currency: &str,
    now: Timestamp,
) -> Result<Recorded, Error> {
    let at = stamp(now);
    let mut recorded = Recorded::default();
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    for account in reported {
        let known = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM connection_accounts
                 WHERE connection_id = ? AND external_account_id = ?) AS "known!: bool""#,
            connection_id,
            account.external_id
        )
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query!(
            "INSERT INTO connection_accounts
               (connection_id, external_account_id, provider_name, institution, currency,
                first_seen_at, last_seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT (connection_id, external_account_id) DO UPDATE SET
               provider_name = excluded.provider_name, institution = excluded.institution,
               currency = excluded.currency, last_seen_at = excluded.last_seen_at",
            connection_id,
            account.external_id,
            account.name,
            account.institution,
            account.currency,
            at
        )
        .execute(&mut *tx)
        .await?;
        if !known {
            recorded.new += 1;
        }
        if account.currency != household_currency {
            recorded.refused.push(account.name.clone());
            continue;
        }
        let attach = sqlx::query_scalar!(
            r#"SELECT a.id AS "id!: i64" FROM accounts a, connection_accounts ca
               WHERE ca.connection_id = ?1 AND ca.external_account_id = ?2
                 AND ca.decision = 'pending'
                 AND a.external_account_id = ?2
                 AND (a.connection_id IS NULL OR a.connection_id = ?1)
                 AND NOT EXISTS (SELECT 1 FROM connection_accounts o WHERE o.account_id = a.id)
               ORDER BY a.id LIMIT 1"#,
            connection_id,
            account.external_id
        )
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(account_id) = attach {
            link(&mut tx, connection_id, &account.external_id, account_id).await?;
            recorded.attached += 1;
        }
    }
    tx.commit().await?;
    Ok(recorded)
}

/// Points account `account_id` at the feed account and marks that tracked.
async fn link(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    connection_id: i64,
    external_id: &str,
    account_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE accounts SET connection_id = ?, external_account_id = ? WHERE id = ?",
        connection_id,
        external_id,
        account_id
    )
    .execute(&mut **tx)
    .await?;
    sqlx::query!(
        "UPDATE connection_accounts SET decision = 'tracked', account_id = ?
         WHERE connection_id = ? AND external_account_id = ?",
        account_id,
        connection_id,
        external_id
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Where a feed account stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedDecision {
    /// Waiting in the prompt.
    Pending,
    Tracked,
    Ignored,
}

/// A feed account, as the prompt shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionAccount {
    pub external_id: String,
    pub name: String,
    pub institution: Option<String>,
    pub currency: String,
    pub decision: FeedDecision,
    /// Its account, once tracked.
    pub account_id: Option<i64>,
    /// For one waiting: a tracked account with the same name and bank whose own provider id has
    /// stopped arriving, the likely one it is the same as *(agent)*.
    pub same_as: Option<i64>,
}

/// Every account the household's connection has reported, in the order first seen; none without
/// a connection.
///
/// # Errors
/// If the database fails or holds a decision it can't read.
pub async fn connection_accounts(pool: &SqlitePool) -> Result<Vec<ConnectionAccount>, Error> {
    let rows = sqlx::query!(
        r#"SELECT ca.external_account_id, ca.provider_name, ca.institution, ca.currency,
             ca.decision, ca.account_id,
             (SELECT old.account_id FROM connection_accounts old
               WHERE ca.decision = 'pending'
                 AND old.connection_id = ca.connection_id AND old.decision = 'tracked'
                 AND old.provider_name = ca.provider_name AND old.institution IS ca.institution
                 AND old.last_seen_at < (SELECT MAX(n.last_seen_at) FROM connection_accounts n
                                         WHERE n.connection_id = ca.connection_id)
               ORDER BY old.last_seen_at DESC LIMIT 1) AS "same_as: i64"
           FROM connection_accounts ca
           WHERE ca.connection_id = (SELECT id FROM connections ORDER BY id LIMIT 1)
           ORDER BY ca.first_seen_at, ca.rowid"#
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(ConnectionAccount {
                decision: match row.decision.as_str() {
                    "pending" => FeedDecision::Pending,
                    "tracked" => FeedDecision::Tracked,
                    "ignored" => FeedDecision::Ignored,
                    _ => {
                        return Err(Error::Unreadable {
                            what: "a feed account decision",
                            value: row.decision,
                        });
                    }
                },
                external_id: row.external_account_id,
                name: row.provider_name,
                institution: row.institution,
                currency: row.currency,
                account_id: row.account_id,
                same_as: row.same_as,
            })
        })
        .collect()
}

/// How to track a feed account.
#[derive(Debug, Clone)]
pub struct Track {
    pub role: Role,
    /// True for a balances-only account.
    pub balances_only: bool,
    /// The account it is the same as, moved onto it; `None` creates a new one.
    pub same_as: Option<i64>,
}

#[derive(Debug, thiserror::Error)]
pub enum TrackError {
    #[error("the connection hasn't reported that account")]
    NotFound,
    #[error("that account is already tracked")]
    AlreadyTracked,
    #[error("that account's currency isn't the household's")]
    ForeignCurrency { currency: String },
    #[error("there's no such account to link it to")]
    NoSuchAccount,
    #[error(transparent)]
    Db(#[from] Error),
}

impl From<sqlx::Error> for TrackError {
    fn from(error: sqlx::Error) -> Self {
        TrackError::Db(Error::Sqlx(error))
    }
}

/// The waiting or ignored feed account `external_id` of the household's connection.
async fn waiting(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    external_id: &str,
) -> Result<(i64, String, Option<String>, String), TrackError> {
    let row = sqlx::query!(
        r#"SELECT connection_id AS "connection_id!: i64", provider_name, institution, currency, decision
           FROM connection_accounts
           WHERE connection_id = (SELECT id FROM connections ORDER BY id LIMIT 1)
             AND external_account_id = ?"#,
        external_id
    )
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(TrackError::NotFound)?;
    if row.decision == "tracked" {
        return Err(TrackError::AlreadyTracked);
    }
    Ok((
        row.connection_id,
        row.provider_name,
        row.institution,
        row.currency,
    ))
}

/// Tracks the feed account `external_id`, waiting or ignored: links it to the account
/// `track.same_as` (which moves onto it with its history and opening balance, leaving the feed
/// account it had, if any, ignored), or creates an account for it, last in the order, named as
/// the bank names it (made unique with a number) and of the type its name suggests. Either way
/// the account takes the role and balances-only chosen. Resolves to the account's id.
///
/// # Errors
/// [`TrackError`]: no such feed account, already tracked, in another currency than
/// `household_currency`, or no account `same_as`; otherwise only if the database fails.
pub async fn track_connection_account(
    pool: &SqlitePool,
    external_id: &str,
    track: &Track,
    household_currency: &str,
) -> Result<i64, TrackError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let (connection_id, name, institution, currency) = waiting(&mut tx, external_id).await?;
    if currency != household_currency {
        return Err(TrackError::ForeignCurrency { currency });
    }
    let role = track.role.as_str();
    let has_transactions = !track.balances_only;
    let account_id = if let Some(id) = track.same_as {
        let exists = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM accounts WHERE id = ?) AS "exists!: bool""#,
            id
        )
        .fetch_one(&mut *tx)
        .await?;
        if !exists {
            return Err(TrackError::NoSuchAccount);
        }
        // The feed account it had, if any, lets go of it.
        sqlx::query!(
            "UPDATE connection_accounts SET decision = 'ignored', account_id = NULL
             WHERE account_id = ?",
            id
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "UPDATE accounts SET role = ?, has_transactions = ? WHERE id = ?",
            role,
            has_transactions,
            id
        )
        .execute(&mut *tx)
        .await?;
        id
    } else {
        let name = free_name(&mut tx, &name).await?;
        let kind = AccountType::guess(&name, track.role).as_str();
        sqlx::query_scalar!(
            r#"INSERT INTO accounts (name, institution, type, role, has_transactions, sort_order)
               VALUES (?, ?, ?, ?, ?, (SELECT COALESCE(MAX(sort_order) + 1, 0) FROM accounts))
               RETURNING id AS "id!: i64""#,
            name,
            institution,
            kind,
            role,
            has_transactions
        )
        .fetch_one(&mut *tx)
        .await?
    };
    link(&mut tx, connection_id, external_id, account_id).await?;
    tx.commit().await?;
    Ok(account_id)
}

/// `name`, or `name 2`, `name 3`… if an account has it already, whatever its case. An empty one
/// is "Bank account" *(agent)*.
async fn free_name(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    name: &str,
) -> Result<String, sqlx::Error> {
    let base = if name.trim().is_empty() {
        "Bank account"
    } else {
        name.trim()
    };
    let mut candidate = base.to_owned();
    for n in 2_u32.. {
        let taken = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM accounts WHERE lower(name) = lower(?)) AS "taken!: bool""#,
            candidate
        )
        .fetch_one(&mut **tx)
        .await?;
        if !taken {
            break;
        }
        candidate = format!("{base} {n}");
    }
    Ok(candidate)
}

/// Ignores the waiting feed account `external_id`: it moves to Ignored, and nothing from it
/// counts.
///
/// # Errors
/// [`TrackError::NotFound`] or [`TrackError::AlreadyTracked`]; otherwise only if the database
/// fails.
pub async fn ignore_connection_account(
    pool: &SqlitePool,
    external_id: &str,
) -> Result<(), TrackError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let (connection_id, ..) = waiting(&mut tx, external_id).await?;
    sqlx::query!(
        "UPDATE connection_accounts SET decision = 'ignored'
         WHERE connection_id = ? AND external_account_id = ?",
        connection_id,
        external_id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save_connection;
    use crate::tests::migrated;

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    fn reported(id: &str, name: &str, currency: &str) -> ReportedAccount {
        ReportedAccount {
            external_id: id.to_owned(),
            name: name.to_owned(),
            institution: Some("First Demo Bank".to_owned()),
            currency: currency.to_owned(),
        }
    }

    fn steady() -> Vec<ReportedAccount> {
        vec![
            reported("ACT-demo-checking", "Demo Everyday Checking", "USD"),
            reported("ACT-demo-card", "Demo Rewards Card", "USD"),
            reported("ACT-demo-savings", "Demo Savings", "USD"),
        ]
    }

    async fn decisions(pool: &SqlitePool) -> Vec<(String, FeedDecision, Option<i64>)> {
        connection_accounts(pool)
            .await
            .unwrap()
            .into_iter()
            .map(|a| (a.external_id, a.decision, a.account_id))
            .collect()
    }

    async fn account(pool: &SqlitePool, id: i64) -> (String, String, String, bool, Option<i64>) {
        sqlx::query_as(
            "SELECT name, type, role, has_transactions, connection_id FROM accounts WHERE id = ?",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn every_reported_account_waits_until_decided() {
        let (_dir, pool) = migrated().await;
        assert_eq!(connection_accounts(&pool).await.unwrap(), []);
        let id = save_connection(&pool, b"sealed").await.unwrap();
        let now = at("2026-10-28T09:00:00Z");
        let recorded = record_connection_accounts(&pool, id, &steady(), "USD", now)
            .await
            .unwrap();
        assert_eq!(
            recorded,
            Recorded {
                new: 3,
                ..Recorded::default()
            }
        );
        let again = record_connection_accounts(&pool, id, &steady(), "USD", now)
            .await
            .unwrap();
        assert_eq!(again.new, 0);
        let listed = connection_accounts(&pool).await.unwrap();
        assert_eq!(listed.len(), 3);
        assert!(
            listed
                .iter()
                .all(|a| a.decision == FeedDecision::Pending && a.same_as.is_none())
        );
        assert_eq!(listed[0].name, "Demo Everyday Checking");
        assert_eq!(listed[0].institution.as_deref(), Some("First Demo Bank"));
    }

    #[tokio::test]
    async fn track_ignore_and_revive() {
        let (_dir, pool) = migrated().await;
        let id = save_connection(&pool, b"sealed").await.unwrap();
        record_connection_accounts(&pool, id, &steady(), "USD", at("2026-10-28T09:00:00Z"))
            .await
            .unwrap();

        let budgeted = Track {
            role: Role::Budgeted,
            balances_only: false,
            same_as: None,
        };
        let checking = track_connection_account(&pool, "ACT-demo-checking", &budgeted, "USD")
            .await
            .unwrap();
        assert_eq!(
            account(&pool, checking).await,
            (
                "Demo Everyday Checking".to_owned(),
                "checking".to_owned(),
                "budgeted".to_owned(),
                true,
                Some(id)
            )
        );
        let card = track_connection_account(&pool, "ACT-demo-card", &budgeted, "USD")
            .await
            .unwrap();
        assert_eq!(account(&pool, card).await.1, "credit_card");
        assert!(matches!(
            track_connection_account(&pool, "ACT-demo-card", &budgeted, "USD").await,
            Err(TrackError::AlreadyTracked)
        ));
        assert!(matches!(
            track_connection_account(&pool, "ACT-nope", &budgeted, "USD").await,
            Err(TrackError::NotFound)
        ));

        ignore_connection_account(&pool, "ACT-demo-savings")
            .await
            .unwrap();
        assert!(matches!(
            ignore_connection_account(&pool, "ACT-demo-card").await,
            Err(TrackError::AlreadyTracked)
        ));
        assert_eq!(
            decisions(&pool).await,
            [
                (
                    "ACT-demo-checking".to_owned(),
                    FeedDecision::Tracked,
                    Some(checking)
                ),
                (
                    "ACT-demo-card".to_owned(),
                    FeedDecision::Tracked,
                    Some(card)
                ),
                ("ACT-demo-savings".to_owned(), FeedDecision::Ignored, None),
            ]
        );

        // Start tracking…: balances only, as a tracking account.
        let tracking = Track {
            role: Role::Tracking,
            balances_only: true,
            same_as: None,
        };
        let savings = track_connection_account(&pool, "ACT-demo-savings", &tracking, "USD")
            .await
            .unwrap();
        assert_eq!(
            account(&pool, savings).await,
            (
                "Demo Savings".to_owned(),
                "savings".to_owned(),
                "tracking".to_owned(),
                false,
                Some(id)
            )
        );
    }

    #[tokio::test]
    async fn a_name_already_taken_gets_a_number() {
        let (_dir, pool) = migrated().await;
        let id = save_connection(&pool, b"sealed").await.unwrap();
        sqlx::query(
            "INSERT INTO accounts (name, type, role) VALUES ('demo savings', 'cash', 'budgeted')",
        )
        .execute(&pool)
        .await
        .unwrap();
        record_connection_accounts(&pool, id, &steady(), "USD", at("2026-10-28T09:00:00Z"))
            .await
            .unwrap();
        let track = Track {
            role: Role::Tracking,
            balances_only: false,
            same_as: None,
        };
        let savings = track_connection_account(&pool, "ACT-demo-savings", &track, "USD")
            .await
            .unwrap();
        assert_eq!(account(&pool, savings).await.0, "Demo Savings 2");
    }

    #[tokio::test]
    async fn an_imported_account_with_the_provider_id_is_attached_at_once() {
        let (_dir, pool) = migrated().await;
        sqlx::query(
            "INSERT INTO accounts (id, name, type, role, external_account_id)
             VALUES (7, 'Checking from Actual', 'checking', 'tracking', 'ACT-demo-checking')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let id = save_connection(&pool, b"sealed").await.unwrap();
        let recorded =
            record_connection_accounts(&pool, id, &steady(), "USD", at("2026-10-28T09:00:00Z"))
                .await
                .unwrap();
        assert_eq!((recorded.new, recorded.attached), (3, 1));
        assert_eq!(
            decisions(&pool).await[0],
            (
                "ACT-demo-checking".to_owned(),
                FeedDecision::Tracked,
                Some(7)
            )
        );
        // Still the role the import chose.
        assert_eq!(
            account(&pool, 7).await,
            (
                "Checking from Actual".to_owned(),
                "checking".to_owned(),
                "tracking".to_owned(),
                true,
                Some(id)
            )
        );
    }

    #[tokio::test]
    async fn another_currency_is_recorded_and_never_tracked() {
        let (_dir, pool) = migrated().await;
        sqlx::query(
            "INSERT INTO accounts (name, type, role, external_account_id)
             VALUES ('Travel', 'credit_card', 'budgeted', 'ACT-demo-travel')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let id = save_connection(&pool, b"sealed").await.unwrap();
        let mut feed = steady();
        feed.push(reported("ACT-demo-travel", "Demo Travel Card", "CAD"));
        let recorded =
            record_connection_accounts(&pool, id, &feed, "USD", at("2026-10-28T09:00:00Z"))
                .await
                .unwrap();
        assert_eq!(recorded.refused, ["Demo Travel Card"]);
        assert_eq!(recorded.attached, 0);
        let track = Track {
            role: Role::Budgeted,
            balances_only: false,
            same_as: None,
        };
        let refused = track_connection_account(&pool, "ACT-demo-travel", &track, "USD").await;
        assert!(
            matches!(refused, Err(TrackError::ForeignCurrency { ref currency }) if currency == "CAD")
        );
    }

    #[tokio::test]
    async fn same_as_moves_an_account_with_its_rows_onto_the_new_id() {
        let (_dir, pool) = migrated().await;
        let id = save_connection(&pool, b"sealed").await.unwrap();
        record_connection_accounts(&pool, id, &steady(), "USD", at("2026-10-27T09:00:00Z"))
            .await
            .unwrap();
        let budgeted = Track {
            role: Role::Budgeted,
            balances_only: false,
            same_as: None,
        };
        let checking = track_connection_account(&pool, "ACT-demo-checking", &budgeted, "USD")
            .await
            .unwrap();
        sqlx::query(
            "UPDATE accounts SET opening_balance_minor = 10000, opening_date = '2026-09-30' WHERE id = ?",
        )
        .bind(checking)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO transactions (account_id, date, amount_minor, status, source, raw_description, dedupe_key)
             VALUES (?, '2026-10-20', -500, 'posted', 'provider', 'DEMO CAFE', 'k')",
        )
        .bind(checking)
        .execute(&pool)
        .await
        .unwrap();

        // The bank was re-linked: the checking account comes back under a new id.
        let mut relinked = steady();
        relinked[0].external_id = "ACT-demo-checking-2".to_owned();
        record_connection_accounts(&pool, id, &relinked, "USD", at("2026-10-28T09:00:00Z"))
            .await
            .unwrap();
        let listed = connection_accounts(&pool).await.unwrap();
        let new = listed
            .iter()
            .find(|a| a.external_id == "ACT-demo-checking-2")
            .unwrap();
        assert_eq!(new.decision, FeedDecision::Pending);
        assert_eq!(new.same_as, Some(checking));
        // The others still arrive, so they suggest nothing.
        assert!(
            listed
                .iter()
                .filter(|a| a.external_id != "ACT-demo-checking-2")
                .all(|a| a.same_as.is_none())
        );

        let same = Track {
            same_as: Some(checking),
            ..budgeted.clone()
        };
        assert_eq!(
            track_connection_account(&pool, "ACT-demo-checking-2", &same, "USD")
                .await
                .unwrap(),
            checking
        );
        let moved: (Option<String>, Option<i64>, Option<i64>, i64) = sqlx::query_as(
            "SELECT external_account_id, connection_id, opening_balance_minor,
               (SELECT COUNT(*) FROM transactions WHERE account_id = accounts.id)
             FROM accounts WHERE id = ?",
        )
        .bind(checking)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            moved,
            (
                Some("ACT-demo-checking-2".to_owned()),
                Some(id),
                Some(10000),
                1
            )
        );
        let after = decisions(&pool).await;
        assert!(after.contains(&("ACT-demo-checking".to_owned(), FeedDecision::Ignored, None)));
        assert!(after.contains(&(
            "ACT-demo-checking-2".to_owned(),
            FeedDecision::Tracked,
            Some(checking)
        )));
        assert!(matches!(
            track_connection_account(
                &pool,
                "ACT-demo-card",
                &Track {
                    same_as: Some(999),
                    ..budgeted
                },
                "USD"
            )
            .await,
            Err(TrackError::NoSuchAccount)
        ));
    }

    #[tokio::test]
    async fn a_manual_account_can_be_the_same_as_a_feed_account() {
        let (_dir, pool) = migrated().await;
        sqlx::query("INSERT INTO accounts (id, name, type, role) VALUES (3, 'Wallet card', 'credit_card', 'budgeted')")
            .execute(&pool)
            .await
            .unwrap();
        let id = save_connection(&pool, b"sealed").await.unwrap();
        record_connection_accounts(&pool, id, &steady(), "USD", at("2026-10-28T09:00:00Z"))
            .await
            .unwrap();
        let track = Track {
            role: Role::Budgeted,
            balances_only: false,
            same_as: Some(3),
        };
        track_connection_account(&pool, "ACT-demo-card", &track, "USD")
            .await
            .unwrap();
        assert_eq!(account(&pool, 3).await.4, Some(id));
    }

    #[tokio::test]
    async fn disconnecting_forgets_the_feed_accounts_and_reconnecting_reattaches() {
        let (_dir, pool) = migrated().await;
        let id = save_connection(&pool, b"sealed").await.unwrap();
        record_connection_accounts(&pool, id, &steady(), "USD", at("2026-10-28T09:00:00Z"))
            .await
            .unwrap();
        let track = Track {
            role: Role::Budgeted,
            balances_only: false,
            same_as: None,
        };
        let checking = track_connection_account(&pool, "ACT-demo-checking", &track, "USD")
            .await
            .unwrap();
        crate::disconnect(&pool).await.unwrap();
        assert_eq!(connection_accounts(&pool).await.unwrap(), []);

        let id = save_connection(&pool, b"sealed-again").await.unwrap();
        let recorded =
            record_connection_accounts(&pool, id, &steady(), "USD", at("2026-10-29T09:00:00Z"))
                .await
                .unwrap();
        assert_eq!(recorded.attached, 1);
        assert_eq!(account(&pool, checking).await.4, Some(id));
    }
}
