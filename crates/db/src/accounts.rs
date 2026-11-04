//! Accounts (docs/spec/m1-ledger-import.md#accounts). Read-only in milestone 1, where the Actual
//! importer creates them.

use domain::accounts::{AccountType, Opening, Role, app_balance};
use domain::categories::Name;
use domain::{Date, Money};

use crate::{Error, SqlitePool};

/// An account, as far as the importer's Same as choice needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: i64,
    pub name: String,
    /// The provider's id for it, or Actual's bank-sync id after an import.
    pub external_account_id: Option<String>,
}

/// Every account, closed ones included, in order.
///
/// # Errors
/// If the database fails.
pub async fn accounts(pool: &SqlitePool) -> Result<Vec<Account>, Error> {
    let accounts = sqlx::query_as!(
        Account,
        r#"SELECT id AS "id!: i64", name, external_account_id
           FROM accounts ORDER BY sort_order, id"#
    )
    .fetch_all(pool)
    .await?;
    Ok(accounts)
}

/// An account as the Accounts screen shows it: what it is, and its balances, the one shown (its
/// latest snapshot) and the app's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountBalance {
    pub id: i64,
    pub name: String,
    pub institution: Option<String>,
    pub kind: AccountType,
    pub role: Role,
    pub closed: bool,
    /// Whether it counts in net worth.
    pub in_net_worth: bool,
    /// False for a balances-only account, which holds balance snapshots and no transactions.
    pub has_transactions: bool,
    /// `None` for a balances-only account.
    pub opening: Option<Opening>,
    /// How many transactions it has.
    pub rows: u64,
    /// The date of its latest transaction; `None` without one.
    pub last_transaction: Option<Date>,
    /// Its opening balance plus its posted transactions; `None` without an opening balance.
    pub app_balance: Option<Money>,
    /// Its latest balance snapshot, the balance shown; `None` before it has one.
    pub snapshot: Option<Money>,
}

/// Every account's balances, closed ones included, in order.
///
/// # Errors
/// If the database fails, or holds a date, a sum, a type or a role it can't read.
pub async fn account_balances(pool: &SqlitePool) -> Result<Vec<AccountBalance>, Error> {
    balances(pool, None).await
}

/// One account's balances; `None` if there's no such account.
///
/// # Errors
/// As [`account_balances`].
pub async fn account_balance(pool: &SqlitePool, id: i64) -> Result<Option<AccountBalance>, Error> {
    Ok(balances(pool, Some(id)).await?.into_iter().next())
}

/// Every account's balances, or only account `only`'s.
async fn balances(pool: &SqlitePool, only: Option<i64>) -> Result<Vec<AccountBalance>, Error> {
    let found = sqlx::query!(
        r#"SELECT a.id AS "id!: i64", a.name, a.institution, a.type AS kind, a.role,
             a.closed AS "closed: bool", a.in_net_worth AS "in_net_worth: bool", a.has_transactions AS "has_transactions: bool",
             a.opening_balance_minor, a.opening_date,
             (SELECT COUNT(*) FROM transactions t WHERE t.account_id = a.id) AS "rows!: i64",
             (SELECT MAX(t.date) FROM transactions t WHERE t.account_id = a.id) AS "last: String",
             (SELECT COALESCE(SUM(t.amount_minor), 0) FROM transactions t
               WHERE t.account_id = a.id AND t.status = 'posted') AS "posted!: i64",
             (SELECT s.balance_minor FROM balance_snapshots s WHERE s.account_id = a.id
               ORDER BY s.as_of DESC LIMIT 1) AS "snapshot: i64"
           FROM accounts a WHERE ?1 IS NULL OR a.id = ?1 ORDER BY a.sort_order, a.id"#,
        only
    )
    .fetch_all(pool)
    .await?;
    found
        .into_iter()
        .map(|account| {
            let opening = match (account.opening_balance_minor, account.opening_date) {
                (Some(balance), Some(date)) => Some(Opening {
                    balance: Money::from_minor(balance),
                    date: read_date(date)?,
                }),
                _ => None,
            };
            let app_balance = match opening {
                Some(opening) => Some(
                    app_balance(opening.balance, [Money::from_minor(account.posted)]).ok_or_else(
                        || Error::Unreadable {
                            what: "a balance",
                            value: account.name.clone(),
                        },
                    )?,
                ),
                None => None,
            };
            Ok(AccountBalance {
                id: account.id,
                kind: AccountType::from_stored(&account.kind).ok_or(Error::Unreadable {
                    what: "an account type",
                    value: account.kind,
                })?,
                role: Role::from_stored(&account.role).ok_or(Error::Unreadable {
                    what: "an account role",
                    value: account.role,
                })?,
                name: account.name,
                institution: account.institution,
                closed: account.closed,
                in_net_worth: account.in_net_worth,
                has_transactions: account.has_transactions,
                opening,
                rows: u64::try_from(account.rows).unwrap_or_default(),
                last_transaction: account.last.map(read_date).transpose()?,
                app_balance,
                snapshot: account.snapshot.map(Money::from_minor),
            })
        })
        .collect()
}

/// What an edit changes about an account; `None` leaves that part as it is.
#[derive(Debug, Clone, Default)]
pub struct AccountChange {
    pub name: Option<Name>,
    /// `Some(None)` clears the institution.
    pub institution: Option<Option<String>>,
    pub kind: Option<AccountType>,
    pub role: Option<Role>,
    /// True for a balances-only account.
    pub balances_only: Option<bool>,
    pub in_net_worth: Option<bool>,
    pub closed: Option<bool>,
}

#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error("there's no such account")]
    NotFound,
    #[error("another account has the name, whatever its case")]
    NameTaken,
    #[error("the order doesn't list every account exactly once")]
    Order,
    #[error(transparent)]
    Db(#[from] Error),
}

impl From<sqlx::Error> for AccountError {
    fn from(error: sqlx::Error) -> Self {
        AccountError::Db(Error::Sqlx(error))
    }
}

/// Changes an account.
///
/// # Errors
/// [`AccountError::NotFound`] if there's no such account, [`AccountError::NameTaken`] if another
/// account has the name in any case; otherwise only if the database fails.
pub async fn update_account(
    pool: &SqlitePool,
    id: i64,
    change: &AccountChange,
) -> Result<(), AccountError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let name = change.name.as_ref().map(Name::as_str);
    if let Some(name) = name {
        let taken = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM accounts WHERE lower(name) = lower(?1) AND id <> ?2) AS "taken!: bool""#,
            name,
            id
        )
        .fetch_one(&mut *tx)
        .await?;
        if taken {
            return Err(AccountError::NameTaken);
        }
    }
    let set_institution = change.institution.is_some();
    let institution = change.institution.as_ref().and_then(Option::as_deref);
    let kind = change.kind.map(AccountType::as_str);
    let role = change.role.map(Role::as_str);
    let has_transactions = change.balances_only.map(|only| !only);
    let updated = sqlx::query!(
        "UPDATE accounts SET name = coalesce(?1, name),
           institution = CASE WHEN ?2 THEN ?3 ELSE institution END,
           type = coalesce(?4, type), role = coalesce(?5, role),
           has_transactions = coalesce(?6, has_transactions),
           in_net_worth = coalesce(?7, in_net_worth), closed = coalesce(?8, closed)
         WHERE id = ?9",
        name,
        set_institution,
        institution,
        kind,
        role,
        has_transactions,
        change.in_net_worth,
        change.closed,
        id
    )
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AccountError::NotFound);
    }
    tx.commit().await?;
    Ok(())
}

/// Puts the accounts in the given order.
///
/// # Errors
/// [`AccountError::Order`] if `ids` isn't every account there is exactly once, leaving the order
/// as it was; otherwise only if the database fails.
pub async fn set_account_order(pool: &SqlitePool, ids: &[i64]) -> Result<(), AccountError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let mut have = sqlx::query_scalar!(r#"SELECT id AS "id!: i64" FROM accounts"#)
        .fetch_all(&mut *tx)
        .await?;
    let mut want = ids.to_vec();
    have.sort_unstable();
    want.sort_unstable();
    if have != want {
        return Err(AccountError::Order);
    }
    for (order, id) in (0_i64..).zip(ids) {
        sqlx::query!("UPDATE accounts SET sort_order = ? WHERE id = ?", order, id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

fn read_date(date: String) -> Result<Date, Error> {
    date.parse::<Date>().map_err(|_| Error::Unreadable {
        what: "a date",
        value: date,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::migrated;

    #[tokio::test]
    async fn lists_every_account_in_order() {
        let (_dir, pool) = migrated().await;
        assert_eq!(accounts(&pool).await.unwrap(), []);
        sqlx::query(
            "INSERT INTO accounts (name, type, role, sort_order, external_account_id, closed) VALUES
               ('Savings', 'savings', 'tracking', 2, NULL, 0),
               ('Checking', 'checking', 'budgeted', 1, 'fake-bank-0001', 0),
               ('Old card', 'credit_card', 'budgeted', 2, NULL, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let listed: Vec<(String, Option<String>)> = accounts(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|a| (a.name, a.external_account_id))
            .collect();
        assert_eq!(
            listed,
            [
                ("Checking".to_owned(), Some("fake-bank-0001".to_owned())),
                ("Savings".to_owned(), None),
                ("Old card".to_owned(), None),
            ]
        );
    }

    #[tokio::test]
    async fn reads_each_accounts_balances_and_figures() {
        let (_dir, pool) = migrated().await;
        assert_eq!(account_balances(&pool).await.unwrap(), []);
        sqlx::query(
            "INSERT INTO accounts (id, name, institution, type, role, has_transactions, closed,
               opening_balance_minor, opening_date) VALUES
               (1, 'Checking', 'Fake Bank', 'checking', 'budgeted', 1, 0, 10000, '2025-12-31'),
               (2, 'Brokerage', NULL, 'investment', 'tracking', 0, 1, NULL, NULL);
             INSERT INTO transactions
               (account_id, date, amount_minor, status, raw_description, source, dedupe_key) VALUES
               (1, '2026-01-05', -2500, 'posted', 'a', 'manual', 'a'),
               (1, '2026-02-01', 1000, 'posted', 'b', 'manual', 'b'),
               (1, '2026-02-03', -300, 'pending', 'c', 'manual', 'c');
             INSERT INTO balance_snapshots (account_id, as_of, balance_minor, source) VALUES
               (1, '2026-02-01T00:00:00.000Z', 8500, 'import'),
               (2, '2026-01-01T00:00:00.000Z', 400000, 'import'),
               (2, '2026-03-01T00:00:00.000Z', 410000, 'import')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let date = |text: &str| text.parse::<Date>().unwrap();
        let checking = AccountBalance {
            id: 1,
            in_net_worth: true,
            name: "Checking".to_owned(),
            institution: Some("Fake Bank".to_owned()),
            kind: AccountType::Checking,
            role: Role::Budgeted,
            closed: false,
            has_transactions: true,
            opening: Some(Opening {
                balance: Money::from_minor(10_000),
                date: date("2025-12-31"),
            }),
            rows: 3,
            last_transaction: Some(date("2026-02-03")),
            // Pending rows aren't in it.
            app_balance: Some(Money::from_minor(8_500)),
            snapshot: Some(Money::from_minor(8_500)),
        };
        let brokerage = AccountBalance {
            id: 2,
            in_net_worth: true,
            name: "Brokerage".to_owned(),
            institution: None,
            kind: AccountType::Investment,
            role: Role::Tracking,
            closed: true,
            has_transactions: false,
            opening: None,
            rows: 0,
            last_transaction: None,
            app_balance: None,
            // The latest snapshot.
            snapshot: Some(Money::from_minor(410_000)),
        };
        assert_eq!(
            account_balances(&pool).await.unwrap(),
            [checking.clone(), brokerage.clone()]
        );
        assert_eq!(account_balance(&pool, 2).await.unwrap(), Some(brokerage));
        assert_eq!(account_balance(&pool, 3).await.unwrap(), None);
    }

    #[tokio::test]
    async fn the_schema_holds_its_rules() {
        let (_dir, pool) = migrated().await;
        let insert = |sql: &'static str| sqlx::query(sql).execute(&pool);
        insert("INSERT INTO accounts (name, type, role) VALUES ('A', 'checking', 'budgeted')")
            .await
            .unwrap();
        for bad in [
            // A taken name, an unknown type or role, and half an opening balance.
            "INSERT INTO accounts (name, type, role) VALUES ('A', 'checking', 'budgeted')",
            "INSERT INTO accounts (name, type, role) VALUES ('B', 'wallet', 'budgeted')",
            "INSERT INTO accounts (name, type, role) VALUES ('C', 'cash', 'saved')",
            "INSERT INTO accounts (name, type, role, opening_balance_minor) VALUES ('D', 'cash', 'tracking', 100)",
        ] {
            assert!(insert(bad).await.is_err(), "{bad}");
        }
        // No connections table yet, so any connection id goes for now.
        insert("INSERT INTO accounts (name, type, role, connection_id) VALUES ('E', 'other', 'tracking', 7)")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn edits_an_account_and_refuses_a_name_taken_in_any_case() {
        let (_dir, pool) = migrated().await;
        sqlx::query(
            "INSERT INTO accounts (id, name, type, role) VALUES
               (1, 'Checking', 'checking', 'budgeted'), (2, 'Savings', 'savings', 'budgeted')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let taken = AccountChange {
            name: Some(Name::new("SAVINGS").unwrap()),
            ..AccountChange::default()
        };
        assert!(matches!(
            update_account(&pool, 1, &taken).await,
            Err(AccountError::NameTaken)
        ));
        let change = AccountChange {
            name: Some(Name::new("CHECKING").unwrap()),
            institution: Some(Some("Fake Bank".to_owned())),
            kind: Some(AccountType::Cash),
            role: Some(Role::Tracking),
            balances_only: Some(true),
            in_net_worth: Some(false),
            closed: Some(true),
        };
        update_account(&pool, 1, &change).await.unwrap();
        let account = account_balance(&pool, 1).await.unwrap().unwrap();
        assert_eq!(account.name, "CHECKING");
        assert_eq!(account.institution.as_deref(), Some("Fake Bank"));
        assert_eq!(account.kind, AccountType::Cash);
        assert_eq!(account.role, Role::Tracking);
        assert!(!account.has_transactions && !account.in_net_worth && account.closed);
        let clear = AccountChange {
            institution: Some(None),
            ..AccountChange::default()
        };
        update_account(&pool, 1, &clear).await.unwrap();
        let account = account_balance(&pool, 1).await.unwrap().unwrap();
        assert_eq!(account.institution, None);
        assert_eq!(account.name, "CHECKING");
        assert!(matches!(
            update_account(&pool, 9, &clear).await,
            Err(AccountError::NotFound)
        ));
    }

    #[tokio::test]
    async fn orders_accounts_only_by_listing_every_one_once() {
        let (_dir, pool) = migrated().await;
        sqlx::query(
            "INSERT INTO accounts (id, name, type, role) VALUES
               (1, 'A', 'checking', 'budgeted'), (2, 'B', 'savings', 'budgeted')",
        )
        .execute(&pool)
        .await
        .unwrap();
        for bad in [&[1_i64][..], &[1, 1], &[1, 2, 3]] {
            assert!(matches!(
                set_account_order(&pool, bad).await,
                Err(AccountError::Order)
            ));
        }
        set_account_order(&pool, &[2, 1]).await.unwrap();
        let ids: Vec<i64> = accounts(&pool)
            .await
            .unwrap()
            .iter()
            .map(|a| a.id)
            .collect();
        assert_eq!(ids, [2, 1]);
    }
}
