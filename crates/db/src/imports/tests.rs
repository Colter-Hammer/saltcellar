use domain::accounts::{AccountType, Role};
use domain::import::{LinkedTransfer, NewAccount, SplitLine};

use super::*;
use crate::tests::set_up;
use crate::{CategoryError, account_balances, delete_category};

/// A transaction's columns, as the tests read them back.
type Txn = (
    String,
    i64,
    String,
    String,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn date(text: &str) -> Date {
    text.parse().unwrap()
}

fn new_account(name: &str, balances_only: bool) -> Target {
    Target::New(NewAccount {
        name: name.to_owned(),
        kind: AccountType::Checking,
        role: Role::Budgeted,
        balances_only,
        closed: false,
        bank_id: Some(format!("fake-bank-{name}")),
    })
}

fn row(import_ref: &str, day: &str, amount: i64) -> Row {
    Row {
        import_ref: import_ref.to_owned(),
        date: date(day),
        amount: Money::from_minor(amount),
        raw_description: String::new(),
        external_id: None,
        payee: None,
        alias: None,
        memo: None,
        tags: Vec::new(),
        disposition: Disposition::Uncategorized,
        source_category: None,
    }
}

fn planned(target: Target, balance: i64, rows: Vec<Row>) -> PlannedAccount {
    PlannedAccount {
        actual_name: "Actual account".to_owned(),
        target,
        balance: Money::from_minor(balance),
        as_of: rows.iter().map(|row| row.date).max(),
        rows,
    }
}

/// The login `set_up` makes, and a category, Groceries.
async fn login_and_category(pool: &SqlitePool) -> (i64, i64) {
    let user: i64 = sqlx::query_scalar("SELECT id FROM users")
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO category_groups (id, name) VALUES (1, 'Food')")
        .execute(pool)
        .await
        .unwrap();
    let category: i64 = sqlx::query_scalar(
        "INSERT INTO categories (group_id, name) VALUES (1, 'Groceries') RETURNING id",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    (user, category)
}

async fn commit(pool: &SqlitePool, plan: &Plan, user: i64) -> Result<Committed, ImportError> {
    commit_actual_import(
        pool,
        ActualCommit {
            plan,
            file_name: "export.zip",
            imported_by: user,
            today: date("2026-10-30"),
        },
    )
    .await
}

async fn count(pool: &SqlitePool, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .unwrap()
}

/// A plan touching every table: checking with a mapped, an undecided and a tagged row, a payee
/// seen twice, savings with no rows, and a balances-only brokerage.
fn full_plan(category: i64) -> Plan {
    let mut groceries = row("t1", "2025-03-02", -1_250);
    groceries.payee = Some("Corner Grocer".to_owned());
    groceries.alias = Some("corner grocer".to_owned());
    groceries.raw_description = "CORNER GROCER".to_owned();
    groceries.external_id = Some("bank-1".to_owned());
    groceries.memo = Some("Weekly #food".to_owned());
    groceries.tags = vec!["food".to_owned()];
    groceries.disposition = Disposition::Mapped {
        category_id: category,
        reason: "From Actual: Groceries".to_owned(),
    };
    groceries.source_category = Some("Groceries".to_owned());
    let mut again = row("t2", "2025-03-01", -750);
    again.payee = Some("Corner Grocer".to_owned());
    again.alias = Some("corner grocer 2".to_owned());
    again.tags = vec!["food".to_owned()];
    again.disposition = Disposition::Undecided;
    again.source_category = Some("Fuel".to_owned());
    let mut transfer = row("t3", "2025-03-05", -5_000);
    transfer.disposition = Disposition::Transfer;
    Plan {
        accounts: vec![
            planned(
                new_account("Checking", false),
                100_000,
                vec![groceries, again, transfer],
            ),
            planned(new_account("Savings", false), 40_000, Vec::new()),
            PlannedAccount {
                as_of: Some(date("2025-02-01")),
                ..planned(new_account("Brokerage", true), 1_100_000, Vec::new())
            },
        ],
        skipped: 4,
        transfers: Vec::new(),
    }
}

#[tokio::test]
async fn a_commit_writes_its_accounts_payees_and_batch() {
    let (_dir, pool) = set_up().await;
    let (user, category) = login_and_category(&pool).await;
    sqlx::query("INSERT INTO payees (name) VALUES ('Corner Grocer')")
        .execute(&pool)
        .await
        .unwrap();
    assert!(!actual_import_in_place(&pool).await.unwrap());

    let committed = commit(&pool, &full_plan(category), user).await.unwrap();
    let names: Vec<(&str, bool, u64)> = committed
        .accounts
        .iter()
        .map(|a| (a.name.as_str(), a.created, a.rows))
        .collect();
    assert_eq!(
        names,
        [
            ("Checking", true, 3),
            ("Savings", true, 0),
            ("Brokerage", true, 0)
        ]
    );
    assert!(actual_import_in_place(&pool).await.unwrap());

    let batch: (String, String, i64, i64, i64, Option<i64>) = sqlx::query_as(
        "SELECT format, file_name, added, matched, skipped, imported_by FROM import_batches WHERE id = ?",
    )
    .bind(committed.batch_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        batch,
        ("actual".into(), "export.zip".into(), 3, 0, 4, Some(user))
    );

    // The payee there already is used, and both aliases point at it.
    assert_eq!(count(&pool, "payees").await, 1);
    let aliases: Vec<(String, String)> = sqlx::query_as(
        "SELECT a.normalized_text, p.name FROM payee_aliases a JOIN payees p ON p.id = a.payee_id
         ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        aliases,
        [
            ("corner grocer".into(), "Corner Grocer".into()),
            ("corner grocer 2".into(), "Corner Grocer".into())
        ]
    );
}

#[tokio::test]
async fn a_commit_writes_each_row_with_its_tags_and_batch_row() {
    let (_dir, pool) = set_up().await;
    let (user, category) = login_and_category(&pool).await;
    let committed = commit(&pool, &full_plan(category), user).await.unwrap();

    let rows: Vec<Txn> = sqlx::query_as(
        "SELECT date, amount_minor, status, source, external_id, category_id, categorized_by,
                categorization_reason, memo
         FROM transactions ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        rows[0],
        (
            "2025-03-02".into(),
            -1_250,
            "posted".into(),
            "import".into(),
            Some("bank-1".into()),
            Some(category),
            Some("import".into()),
            Some("From Actual: Groceries".into()),
            Some("Weekly #food".into()),
        )
    );
    assert_eq!(rows[1].5, None);
    assert_eq!(rows[1].6, None);
    let dedupe: String = sqlx::query_scalar("SELECT dedupe_key FROM transactions ORDER BY id")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        dedupe,
        dedupe_key(
            committed.accounts[0].id,
            date("2025-03-02"),
            Money::from_minor(-1_250),
            "CORNER GROCER"
        )
    );
    assert_eq!(count(&pool, "tags").await, 1);
    assert_eq!(count(&pool, "transaction_tags").await, 2);

    let batch_rows: Vec<(String, Option<String>, Option<String>, String)> = sqlx::query_as(
        "SELECT import_ref, source_category, source_disposition, outcome FROM import_batch_rows
         ORDER BY transaction_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        batch_rows,
        [
            (
                "t1".into(),
                Some("Groceries".into()),
                Some("mapped".into()),
                "inserted".into()
            ),
            (
                "t2".into(),
                Some("Fuel".into()),
                Some("undecided".into()),
                "inserted".into()
            ),
            (
                "t3".into(),
                None,
                Some("transfer".into()),
                "inserted".into()
            ),
        ]
    );
}

#[tokio::test]
async fn a_commit_sets_each_accounts_opening_balance_and_snapshot() {
    let (_dir, pool) = set_up().await;
    let (user, category) = login_and_category(&pool).await;
    commit(&pool, &full_plan(category), user).await.unwrap();

    // Checking opens the day before its first row with what's left; savings, with no rows, opens
    // today with all of it; the brokerage only has a snapshot.
    let balances = account_balances(&pool).await.unwrap();
    let opening = |i: usize| {
        balances[i]
            .opening
            .map(|o| (o.balance.minor(), o.date.to_string()))
    };
    assert_eq!(opening(0), Some((107_000, "2025-02-28".into())));
    assert_eq!(opening(1), Some((40_000, "2026-10-30".into())));
    assert_eq!(opening(2), None);
    let shown: Vec<(Option<i64>, Option<i64>, u64)> = balances
        .iter()
        .map(|b| {
            (
                b.app_balance.map(Money::minor),
                b.snapshot.map(Money::minor),
                b.rows,
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            (Some(100_000), Some(100_000), 3),
            (Some(40_000), Some(40_000), 0),
            (None, Some(1_100_000), 0),
        ]
    );
    let snapshots: Vec<(String, String, Option<i64>)> = sqlx::query_as(
        "SELECT as_of, source, app_balance_posted_minor FROM balance_snapshots ORDER BY account_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        snapshots,
        [
            ("2025-03-05".into(), "import".into(), Some(100_000)),
            ("2026-10-30".into(), "import".into(), Some(40_000)),
            ("2025-02-01".into(), "import".into(), None),
        ]
    );
    let has_transactions: Vec<(String, bool, Option<String>)> = sqlx::query_as(
        "SELECT name, has_transactions, external_account_id FROM accounts ORDER BY sort_order",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        has_transactions,
        [
            ("Checking".into(), true, Some("fake-bank-Checking".into())),
            ("Savings".into(), true, Some("fake-bank-Savings".into())),
            (
                "Brokerage".into(),
                false,
                Some("fake-bank-Brokerage".into())
            ),
        ]
    );

    // A second import waits until this one is undone.
    let again = commit(
        &pool,
        &Plan {
            accounts: Vec::new(),
            skipped: 0,
            transfers: Vec::new(),
        },
        user,
    )
    .await;
    assert!(
        matches!(again, Err(ImportError::EarlierImport)),
        "{again:?}"
    );
    sqlx::query("UPDATE import_batches SET undone_at = '2026-10-30T10:00:00.000Z'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(!actual_import_in_place(&pool).await.unwrap());
}

#[tokio::test]
async fn a_failure_midway_leaves_nothing_behind() {
    let (_dir, pool) = set_up().await;
    let (user, category) = login_and_category(&pool).await;
    let mut plan = full_plan(category);
    // A category that isn't there, on the last row of a later account.
    let mut bad = row("t9", "2025-03-01", -1);
    bad.disposition = Disposition::Mapped {
        category_id: category + 100,
        reason: String::new(),
    };
    plan.accounts[1].rows.push(bad);
    let before: Vec<i64> = written_counts(&pool).await;

    let failed = commit(&pool, &plan, user).await;
    assert!(matches!(failed, Err(ImportError::Db(_))), "{failed:?}");
    assert_eq!(written_counts(&pool).await, before);
    assert!(!actual_import_in_place(&pool).await.unwrap());
}

/// The row counts of every table a commit writes.
async fn written_counts(pool: &SqlitePool) -> Vec<i64> {
    let mut counts = Vec::new();
    for table in [
        "accounts",
        "transactions",
        "transaction_tags",
        "tags",
        "payees",
        "payee_aliases",
        "balance_snapshots",
        "import_batches",
        "import_batch_rows",
    ] {
        counts.push(count(pool, table).await);
    }
    counts
}

#[tokio::test]
async fn an_unworkable_balance_is_refused() {
    let (_dir, pool) = set_up().await;
    let (user, _) = login_and_category(&pool).await;
    let plan = Plan {
        accounts: vec![planned(
            new_account("Huge", false),
            i64::MIN,
            vec![row("t1", "2025-01-01", 1)],
        )],
        skipped: 0,
        transfers: Vec::new(),
    };
    let failed = commit(&pool, &plan, user).await;
    assert!(
        matches!(&failed, Err(ImportError::Balance(name)) if name == "Huge"),
        "{failed:?}"
    );
    assert_eq!(count(&pool, "accounts").await, 0);
}

#[tokio::test]
async fn rows_go_into_an_account_that_is_the_same_as_theirs() {
    let (_dir, pool) = set_up().await;
    let (user, _) = login_and_category(&pool).await;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO accounts (name, type, role) VALUES ('Wallet', 'cash', 'budgeted') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let plan = Plan {
        accounts: vec![planned(
            Target::Existing(id),
            300,
            vec![row("t1", "2025-01-02", 100)],
        )],
        skipped: 0,
        transfers: Vec::new(),
    };
    let committed = commit(&pool, &plan, user).await.unwrap();
    assert_eq!(
        committed.accounts,
        [CommittedAccount {
            id,
            name: "Wallet".into(),
            created: false,
            rows: 1
        }]
    );
    let balances = account_balances(&pool).await.unwrap();
    assert_eq!(balances[0].app_balance, Some(Money::from_minor(300)));
    assert_eq!(balances[0].opening.unwrap().date, date("2025-01-01"));
}

#[tokio::test]
async fn a_category_transactions_or_split_lines_use_cant_be_deleted() {
    let (_dir, pool) = set_up().await;
    let (user, category) = login_and_category(&pool).await;
    commit(&pool, &full_plan(category), user).await.unwrap();
    assert!(matches!(
        delete_category(&pool, category).await,
        Err(CategoryError::InUse)
    ));

    sqlx::query("UPDATE transactions SET category_id = NULL")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO transaction_splits (transaction_id, category_id, amount_minor) SELECT MIN(id), ?, -1 FROM transactions")
        .bind(category)
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        delete_category(&pool, category).await,
        Err(CategoryError::InUse)
    ));

    sqlx::query("DELETE FROM transaction_splits")
        .execute(&pool)
        .await
        .unwrap();
    delete_category(&pool, category).await.unwrap();
}

/// Checking with a split of 3 lines over two categories, and a linked transfer to Savings.
fn split_and_transfer_plan(groceries: i64, fuel: i64) -> Plan {
    let mut split = row("s", "2025-03-02", -3_000);
    split.disposition = Disposition::Split {
        lines: vec![
            SplitLine {
                category_id: groceries,
                amount: Money::from_minor(-2_000),
                memo: Some("food".to_owned()),
            },
            SplitLine {
                category_id: fuel,
                amount: Money::from_minor(-500),
                memo: None,
            },
            SplitLine {
                category_id: groceries,
                amount: Money::from_minor(-500),
                memo: None,
            },
        ],
        reason: "From Actual: Groceries, Fuel".to_owned(),
    };
    let mut plain = row("p", "2025-03-03", -700);
    plain.disposition = Disposition::Mapped {
        category_id: groceries,
        reason: "From Actual: Groceries".to_owned(),
    };
    let mut out = row("out", "2025-03-05", -5_000);
    out.disposition = Disposition::Transfer;
    let mut into = row("in", "2025-03-05", 5_000);
    into.disposition = Disposition::Transfer;
    Plan {
        accounts: vec![
            planned(
                new_account("Checking", false),
                10_000,
                vec![split, plain, out],
            ),
            planned(new_account("Savings", false), 20_000, vec![into]),
        ],
        skipped: 0,
        transfers: vec![LinkedTransfer {
            from: "out".to_owned(),
            to: "in".to_owned(),
        }],
    }
}

#[tokio::test]
async fn a_split_comes_in_with_its_lines_and_a_linked_pair_with_one_transfer() {
    let (_dir, pool) = set_up().await;
    let (user, groceries) = login_and_category(&pool).await;
    let fuel: i64 = sqlx::query_scalar(
        "INSERT INTO categories (group_id, name) VALUES (1, 'Fuel') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    commit(&pool, &split_and_transfer_plan(groceries, fuel), user)
        .await
        .unwrap();

    let split = crate::transaction(&pool, 1).await.unwrap().unwrap();
    assert_eq!(split.row.category_id, None);
    assert!(split.row.split);
    assert_eq!(split.row.split_categories, 2);
    assert_eq!(
        split.categorization_reason.as_deref(),
        Some("From Actual: Groceries, Fuel")
    );
    let lines: Vec<(i64, i64, Option<String>)> = split
        .splits
        .iter()
        .map(|l| (l.category_id, l.amount.minor(), l.memo.clone()))
        .collect();
    assert_eq!(
        lines,
        [
            (groceries, -2_000, Some("food".to_owned())),
            (fuel, -500, None),
            (groceries, -500, None)
        ]
    );
    let by: Option<String> =
        sqlx::query_scalar("SELECT categorized_by FROM transactions WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(by.as_deref(), Some("import"));

    // One transfer, confirmed, out of Checking and into Savings, its legs without a category.
    let (from, to, by, state): (String, String, String, String) = sqlx::query_as(
        "SELECT fa.name, ta.name, matched_by, state FROM transfers
           JOIN accounts fa ON fa.id = from_account_id JOIN accounts ta ON ta.id = to_account_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((from.as_str(), to.as_str()), ("Checking", "Savings"));
    assert_eq!((by.as_str(), state.as_str()), ("import", "confirmed"));
    assert_eq!(count(&pool, "transfers").await, 1);
    let legs: Vec<(i64, Option<i64>, Option<String>)> = sqlx::query_as(
        "SELECT amount_minor, category_id, categorized_by FROM transactions
         WHERE id IN (SELECT from_txn_id FROM transfers UNION SELECT to_txn_id FROM transfers)
         ORDER BY amount_minor",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        legs,
        [
            (-5_000, None, Some("transfer".into())),
            (5_000, None, Some("transfer".into()))
        ]
    );

    // The leg names the other account.
    let page = crate::transactions(&pool, &crate::Filter::default(), None, 50)
        .await
        .unwrap();
    let transfers: Vec<(i64, Option<&str>)> = page
        .transactions
        .iter()
        .filter(|t| t.amount.minor().abs() == 5_000)
        .map(|t| (t.amount.minor(), t.transfer_account.as_deref()))
        .collect();
    assert_eq!(transfers.len(), 2);
    assert!(transfers.contains(&(-5_000, Some("Savings"))));
    assert!(transfers.contains(&(5_000, Some("Checking"))));

    // Each amount counts once in the category-amounts view: the split's lines and the plain row,
    // and nothing for the transfer legs or the split's parent.
    let counted: Vec<(Option<i64>, i64)> = sqlx::query_as(
        "SELECT category_id, amount_minor FROM v_category_lines ORDER BY txn_id, amount_minor, category_id DESC",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        counted,
        [
            (Some(groceries), -2_000),
            (Some(fuel), -500),
            (Some(groceries), -500),
            (Some(groceries), -700)
        ]
    );
    let total: i64 = sqlx::query_scalar("SELECT SUM(amount_minor) FROM v_category_lines")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(total, -3_700);
}
