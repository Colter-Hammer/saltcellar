//! The Actual import over rows an account has already
//! (docs/spec/m2-bank-data.md#import-from-actual-becomes-overlap-aware).

use domain::accounts::Role;
use domain::import::SplitLine;

use super::*;
use crate::tests::set_up;
use crate::undo_import;

fn date(text: &str) -> Date {
    text.parse().unwrap()
}

/// A synced row's fields an import may fill, and those it must keep.
type Fields = (
    String,
    i64,
    String,
    String,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
);

struct Setup {
    user: i64,
    account: i64,
    groceries: i64,
    fuel: i64,
}

/// A login, two categories, and a synced checking account with three bank rows from 2026-10-01
/// on: a grocery row (its payee from the feed), a fuel row the household categorized, and a cafe
/// row.
async fn synced(pool: &SqlitePool) -> Setup {
    let user: i64 = sqlx::query_scalar("SELECT id FROM users")
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO category_groups (id, name) VALUES (1, 'Everyday')")
        .execute(pool)
        .await
        .unwrap();
    let category = |name: &'static str| async move {
        sqlx::query_scalar::<_, i64>(
            "INSERT INTO categories (group_id, name) VALUES (1, ?) RETURNING id",
        )
        .bind(name)
        .fetch_one(pool)
        .await
        .unwrap()
    };
    let groceries = category("Groceries").await;
    let fuel = category("Fuel").await;
    sqlx::query(
        "INSERT INTO connections (id, provider, label, secret_encrypted)
         VALUES (1, 'simplefin', 'Demo Bridge', x'00')",
    )
    .execute(pool)
    .await
    .unwrap();
    let account: i64 = sqlx::query_scalar(
        "INSERT INTO accounts (name, type, role, connection_id, external_account_id,
                               opening_balance_minor, opening_date)
         VALUES ('Demo checking', 'checking', 'budgeted', 1, 'ACT-demo-checking', 100000,
                 '2026-09-30')
         RETURNING id",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO payees (id, name) VALUES (1, 'Demo Grocer')")
        .execute(pool)
        .await
        .unwrap();
    bank_rows(pool, account, fuel).await;
    sqlx::query(
        "INSERT INTO balance_snapshots (account_id, as_of, balance_minor, source)
         VALUES (?, '2026-10-27', 86688, 'provider')",
    )
    .bind(account)
    .execute(pool)
    .await
    .unwrap();
    Setup {
        user,
        account,
        groceries,
        fuel,
    }
}

/// The feed's three rows on `account`: groceries with its payee, fuel the household
/// categorized as `fuel`, and a cafe row.
async fn bank_rows(pool: &SqlitePool, account: i64, fuel: i64) {
    for (id, day, amount, text, payee, category, by) in [
        (
            "TRN-groc-1",
            "2026-10-05",
            -8_612,
            "DEMO GROCER #12",
            Some(1),
            None,
            None,
        ),
        (
            "TRN-fuel-1",
            "2026-10-08",
            -4_125,
            "DEMO FUEL STOP",
            None,
            Some(fuel),
            Some("user"),
        ),
        (
            "TRN-cafe-1",
            "2026-10-20",
            -575,
            "DEMO CAFE",
            None,
            None,
            None,
        ),
    ] {
        sqlx::query(
            "INSERT INTO transactions
               (account_id, date, amount_minor, status, payee_id, raw_description, category_id,
                categorized_by, source, external_id, dedupe_key)
             VALUES (?, ?, ?, 'posted', ?, ?, ?, ?, 'provider', ?, ?)",
        )
        .bind(account)
        .bind(day)
        .bind(amount)
        .bind(payee)
        .bind(text)
        .bind(category)
        .bind(by)
        .bind(id)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    }
}

fn actual_row(id: &str, day: &str, amount: i64, bank_id: Option<&str>) -> Row {
    Row {
        import_ref: id.to_owned(),
        date: date(day),
        amount: Money::from_minor(amount),
        raw_description: String::new(),
        external_id: bank_id.map(str::to_owned),
        payee: None,
        alias: None,
        memo: None,
        tags: Vec::new(),
        disposition: Disposition::Uncategorized,
        source_category: None,
    }
}

fn mapped(category_id: i64, name: &str) -> Disposition {
    Disposition::Mapped {
        category_id,
        reason: format!("From Actual: {name}"),
    }
}

/// Actual's copy of the checking account: the three synced rows by id (its dates a day off, as
/// Actual's own sync had them), with categories, notes and payees; and a row older than anything
/// synced.
fn actual_plan(setup: &Setup) -> Plan {
    let mut groceries = actual_row("a-groc", "2026-10-04", -8_612, Some("TRN-groc-1"));
    groceries.raw_description = "CORNER GROCER".to_owned();
    groceries.payee = Some("Corner Grocer".to_owned());
    groceries.alias = Some("corner grocer".to_owned());
    groceries.memo = Some("Weekly shop #food".to_owned());
    groceries.tags = vec!["food".to_owned()];
    groceries.disposition = mapped(setup.groceries, "Groceries");
    groceries.source_category = Some("Groceries".to_owned());
    let mut fuel = actual_row("a-fuel", "2026-10-07", -4_125, Some("TRN-fuel-1"));
    fuel.disposition = mapped(setup.groceries, "Groceries");
    fuel.memo = Some("Road trip".to_owned());
    let mut cafe = actual_row("a-cafe", "2026-10-20", -575, Some("TRN-cafe-1"));
    cafe.disposition = Disposition::Split {
        lines: vec![
            SplitLine {
                category_id: setup.groceries,
                amount: Money::from_minor(-300),
                memo: None,
            },
            SplitLine {
                category_id: setup.fuel,
                amount: Money::from_minor(-275),
                memo: Some("snack".to_owned()),
            },
        ],
        reason: "From Actual: Groceries, Fuel".to_owned(),
    };
    let mut older = actual_row("a-old", "2026-08-15", -2_000, None);
    older.disposition = mapped(setup.groceries, "Groceries");
    older.raw_description = "OLD GROCER".to_owned();
    Plan {
        accounts: vec![PlannedAccount {
            actual_name: "Checking".to_owned(),
            target: Target::Existing(setup.account),
            balance: Money::from_minor(50_000),
            as_of: Some(date("2026-10-20")),
            rows: vec![groceries, fuel, cafe, older],
        }],
        skipped: 0,
        transfers: Vec::new(),
    }
}

async fn commit(
    pool: &SqlitePool,
    plan: &Plan,
    user: i64,
    new_rows: &HashSet<String>,
) -> Committed {
    commit_actual_import(
        pool,
        ActualCommit {
            plan,
            file_name: "overlap.zip",
            imported_by: user,
            today: date("2026-10-28"),
            new_rows,
        },
    )
    .await
    .unwrap()
}

async fn fields(pool: &SqlitePool, external_id: &str) -> Fields {
    sqlx::query_as(
        "SELECT date, amount_minor, status, raw_description, category_id, categorized_by,
                categorization_reason, memo, payee_id
         FROM transactions WHERE external_id = ?",
    )
    .bind(external_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn synced_rows_take_actuals_category_notes_and_payee_and_undo_restores_them_exactly() {
    let (_dir, pool) = set_up().await;
    let setup = synced(&pool).await;
    let ids = ["TRN-groc-1", "TRN-fuel-1", "TRN-cafe-1"];
    let mut before = Vec::new();
    for id in ids {
        before.push(fields(&pool, id).await);
    }

    let committed = commit(&pool, &actual_plan(&setup), setup.user, &HashSet::new()).await;
    assert_eq!(
        (committed.accounts[0].rows, committed.accounts[0].matched),
        (1, 3)
    );
    // No duplicates: the three synced rows and the older one.
    assert_eq!(count(&pool, "SELECT count(*) FROM transactions").await, 4);

    // The grocery row keeps the bank's date, amount, status and text, and takes Actual's
    // category, notes, payee and tag.
    let corner: i64 = count(&pool, "SELECT id FROM payees WHERE name = 'Corner Grocer'").await;
    let groceries = fields(&pool, "TRN-groc-1").await;
    assert_eq!(
        groceries,
        (
            "2026-10-05".to_owned(),
            -8_612,
            "posted".to_owned(),
            "DEMO GROCER #12".to_owned(),
            Some(setup.groceries),
            Some("import".to_owned()),
            Some("From Actual: Groceries".to_owned()),
            Some("Weekly shop #food".to_owned()),
            Some(corner),
        )
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM transaction_tags tt JOIN tags t ON t.id = tt.tag_id
             WHERE t.name = 'food'"
        )
        .await,
        1
    );
    // The household's category on the fuel row stays; Actual's notes still land.
    let fuel = fields(&pool, "TRN-fuel-1").await;
    assert_eq!(
        (fuel.4, fuel.5.as_deref(), fuel.7.as_deref()),
        (Some(setup.fuel), Some("user"), Some("Road trip"))
    );
    // The cafe row takes Actual's split.
    assert_eq!(
        count(&pool, "SELECT count(*) FROM transaction_splits").await,
        2
    );
    let cafe = fields(&pool, "TRN-cafe-1").await;
    assert_eq!((cafe.4, cafe.5.as_deref()), (None, Some("import")));

    // Undo: the older row goes, and the synced rows have exactly what they had.
    undo_import(
        &pool,
        committed.batch_id,
        true,
        "2026-10-28T12:00:00Z".parse().unwrap(),
    )
    .await
    .unwrap();
    let mut after = Vec::new();
    for id in ids {
        after.push(fields(&pool, id).await);
    }
    assert_eq!(after, before);
    assert_eq!(count(&pool, "SELECT count(*) FROM transactions").await, 3);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM transaction_splits").await,
        0
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM transaction_tags").await,
        0
    );
    // What only the import used is tidied away; the feed's payee stays.
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM payees WHERE name = 'Corner Grocer'"
        )
        .await,
        0
    );
    assert_eq!(count(&pool, "SELECT count(*) FROM tags").await, 0);
    assert_eq!(count(&pool, "SELECT count(*) FROM payees").await, 1);
}

#[tokio::test]
async fn a_second_import_of_the_same_export_adds_nothing() {
    let (_dir, pool) = set_up().await;
    let setup = synced(&pool).await;
    let plan = actual_plan(&setup);
    commit(&pool, &plan, setup.user, &HashSet::new()).await;
    let rows = count(&pool, "SELECT count(*) FROM transactions").await;

    let again = commit(&pool, &plan, setup.user, &HashSet::new()).await;
    assert_eq!((again.accounts[0].rows, again.accounts[0].matched), (0, 4));
    assert_eq!(
        count(&pool, "SELECT count(*) FROM transactions").await,
        rows
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM transaction_splits").await,
        2
    );
}

#[tokio::test]
async fn without_an_id_the_file_rule_matches_inside_the_synced_period_and_ties_wait() {
    let (_dir, pool) = set_up().await;
    let setup = synced(&pool).await;
    // A second cafe row as good as the first, so an Actual cafe row without an id ties.
    sqlx::query(
        "INSERT INTO transactions
           (account_id, date, amount_minor, raw_description, source, external_id, dedupe_key)
         VALUES (?, '2026-10-20', -575, 'DEMO KIOSK', 'provider', 'TRN-kiosk-1', 'k')",
    )
    .bind(setup.account)
    .execute(&pool)
    .await
    .unwrap();
    let mut groceries = actual_row("a-groc", "2026-10-06", -8_612, None);
    groceries.disposition = mapped(setup.groceries, "Groceries");
    let mut cafe = actual_row("a-cafe", "2026-10-20", -575, None);
    cafe.disposition = mapped(setup.groceries, "Groceries");
    let plan = Plan {
        accounts: vec![PlannedAccount {
            rows: vec![groceries, cafe],
            ..actual_plan(&setup).accounts.remove(0)
        }],
        skipped: 0,
        transfers: Vec::new(),
    };

    let overlaps = actual_overlaps(&pool, &plan).await.unwrap();
    let grocery = count(
        &pool,
        "SELECT id FROM transactions WHERE external_id = 'TRN-groc-1'",
    )
    .await;
    let first_cafe = count(
        &pool,
        "SELECT id FROM transactions WHERE external_id = 'TRN-cafe-1'",
    )
    .await;
    assert_eq!(
        overlaps,
        [vec![Overlap::Matched(grocery), Overlap::Check(first_cafe)]]
    );

    // Said to be new, the tie is inserted; the matched row is filled.
    let committed = commit(
        &pool,
        &plan,
        setup.user,
        &HashSet::from(["a-cafe".to_owned()]),
    )
    .await;
    assert_eq!(
        (committed.accounts[0].rows, committed.accounts[0].matched),
        (1, 1)
    );
    assert_eq!(fields(&pool, "TRN-groc-1").await.4, Some(setup.groceries));
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM transactions WHERE amount_minor = -575"
        )
        .await,
        3
    );
}

/// Savings tracking the waiting bank account, and a new account taking the bank's name.
fn track_plan() -> Plan {
    let mut interest = actual_row("a-int", "2026-10-21", 318, Some("TRN-int-1"));
    interest.raw_description = "INTEREST PAID".to_owned();
    Plan {
        accounts: vec![
            PlannedAccount {
                actual_name: "Savings".to_owned(),
                target: Target::Track(TrackedAccount {
                    external_id: "ACT-demo-savings".to_owned(),
                    role: Role::Tracking,
                    balances_only: false,
                }),
                balance: Money::from_minor(500_318),
                as_of: Some(date("2026-10-21")),
                rows: vec![interest],
            },
            // A new account taking the bank's name: the tracked one is numbered instead.
            PlannedAccount {
                actual_name: "Other".to_owned(),
                target: Target::New(domain::import::NewAccount {
                    name: "Demo Savings".to_owned(),
                    kind: domain::accounts::AccountType::Savings,
                    role: Role::Budgeted,
                    balances_only: false,
                    closed: false,
                    bank_id: None,
                }),
                balance: Money::ZERO,
                as_of: None,
                rows: Vec::new(),
            },
        ],
        skipped: 0,
        transfers: Vec::new(),
    }
}

#[tokio::test]
async fn a_bank_account_waiting_in_the_prompt_is_tracked_with_the_role_chosen_and_undo_lets_it_wait()
 {
    let (_dir, pool) = set_up().await;
    let setup = synced(&pool).await;
    sqlx::query(
        "INSERT INTO connection_accounts
           (connection_id, external_account_id, provider_name, institution, currency)
         VALUES (1, 'ACT-demo-savings', 'Demo Savings', 'Second Demo Bank', 'USD')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let plan = track_plan();
    let committed = commit(&pool, &plan, setup.user, &HashSet::new()).await;
    let tracked = &committed.accounts[0];
    assert_eq!(
        (tracked.name.as_str(), tracked.created, tracked.rows),
        ("Demo Savings 2", true, 1)
    );
    let account: (String, String, String, Option<String>) = sqlx::query_as(
        "SELECT role, institution, external_account_id,
                (SELECT decision FROM connection_accounts WHERE account_id = accounts.id)
         FROM accounts WHERE id = ?",
    )
    .bind(tracked.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        account,
        (
            "tracking".to_owned(),
            "Second Demo Bank".to_owned(),
            "ACT-demo-savings".to_owned(),
            Some("tracked".to_owned()),
        )
    );

    // Tracked already, it can't be tracked again.
    let again = commit_actual_import(
        &pool,
        ActualCommit {
            plan: &Plan {
                accounts: vec![plan.accounts[0].clone()],
                skipped: 0,
                transfers: Vec::new(),
            },
            file_name: "overlap.zip",
            imported_by: setup.user,
            today: date("2026-10-28"),
            new_rows: &HashSet::new(),
        },
    )
    .await;
    assert!(
        matches!(&again, Err(ImportError::NotWaiting(id)) if id == "ACT-demo-savings"),
        "{again:?}"
    );

    undo_import(
        &pool,
        committed.batch_id,
        true,
        "2026-10-28T12:00:00Z".parse().unwrap(),
    )
    .await
    .unwrap();
    let waiting: (String, Option<i64>) = sqlx::query_as(
        "SELECT decision, account_id FROM connection_accounts
         WHERE external_account_id = 'ACT-demo-savings'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(waiting, ("pending".to_owned(), None));
}

#[tokio::test]
async fn a_row_the_household_entered_keeps_its_payee() {
    let (_dir, pool) = set_up().await;
    let setup = synced(&pool).await;
    sqlx::query(
        "INSERT INTO transactions
           (account_id, date, amount_minor, payee_id, raw_description, source, dedupe_key)
         VALUES (?, '2026-10-12', -999, 1, 'Demo Grocer', 'manual', 'k')",
    )
    .bind(setup.account)
    .execute(&pool)
    .await
    .unwrap();
    let mut row = actual_row("a-shop", "2026-10-12", -999, None);
    row.payee = Some("Corner Grocer".to_owned());
    row.disposition = mapped(setup.groceries, "Groceries");
    let plan = Plan {
        accounts: vec![PlannedAccount {
            rows: vec![row],
            ..actual_plan(&setup).accounts.remove(0)
        }],
        skipped: 0,
        transfers: Vec::new(),
    };
    commit(&pool, &plan, setup.user, &HashSet::new()).await;
    let (payee, category): (Option<i64>, Option<i64>) =
        sqlx::query_as("SELECT payee_id, category_id FROM transactions WHERE source = 'manual'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((payee, category), (Some(1), Some(setup.groceries)));
}
