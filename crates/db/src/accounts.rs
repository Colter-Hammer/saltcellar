//! Accounts (docs/spec/m1-ledger-import.md#accounts). Read-only in milestone 1, where the Actual
//! importer creates them.

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
