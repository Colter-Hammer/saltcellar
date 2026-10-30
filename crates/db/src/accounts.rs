//! Accounts (docs/spec/m1-ledger-import.md#accounts). Read-only in milestone 1, where the Actual
//! importer creates them.

use domain::accounts::{Opening, app_balance};
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

/// An account's balances: the one shown (its latest snapshot) and the app's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountBalance {
    pub id: i64,
    pub name: String,
    /// `None` for a balances-only account.
    pub opening: Option<Opening>,
    /// How many transactions it has.
    pub rows: u64,
    /// Its opening balance plus its posted transactions; `None` without an opening balance.
    pub app_balance: Option<Money>,
    /// Its latest balance snapshot, the balance shown; `None` before it has one.
    pub snapshot: Option<Money>,
}

/// Every account's balances, in order.
///
/// # Errors
/// If the database fails, or holds a date or a sum it can't read.
pub async fn account_balances(pool: &SqlitePool) -> Result<Vec<AccountBalance>, Error> {
    let found = sqlx::query!(
        r#"SELECT a.id AS "id!: i64", a.name, a.opening_balance_minor, a.opening_date,
             (SELECT COUNT(*) FROM transactions t WHERE t.account_id = a.id) AS "rows!: i64",
             (SELECT COALESCE(SUM(t.amount_minor), 0) FROM transactions t
               WHERE t.account_id = a.id AND t.status = 'posted') AS "posted!: i64",
             (SELECT s.balance_minor FROM balance_snapshots s WHERE s.account_id = a.id
               ORDER BY s.as_of DESC LIMIT 1) AS "snapshot: i64"
           FROM accounts a ORDER BY a.sort_order, a.id"#
    )
    .fetch_all(pool)
    .await?;
    found
        .into_iter()
        .map(|account| {
            let opening = match (account.opening_balance_minor, account.opening_date) {
                (Some(balance), Some(date)) => Some(Opening {
                    balance: Money::from_minor(balance),
                    date: date.parse::<Date>().map_err(|_| Error::Unreadable {
                        what: "an opening date",
                        value: date,
                    })?,
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
                name: account.name,
                opening,
                rows: u64::try_from(account.rows).unwrap_or_default(),
                app_balance,
                snapshot: account.snapshot.map(Money::from_minor),
            })
        })
        .collect()
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
}
