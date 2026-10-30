//! The committed fake exports in `fixtures/`: what the reader makes of them, and that they still
//! match what the builder ([`crate::fake`]) writes.

use std::path::{Path, PathBuf};

use crate::{Export, ReadError, Summary, fake, unpack};
use domain::actual::{self, Payee};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

async fn summary_of(zip: &Path) -> Result<Summary, ReadError> {
    let dir = tempfile::tempdir().unwrap();
    unpack(zip, dir.path())?;
    let mut export = Export::open(dir.path()).await?;
    let summary = export.summary().await;
    export.close().await;
    summary
}

fn as_expected(summary: &Summary) -> fake::Expected {
    let (first, last) = summary.dates.unwrap();
    fake::Expected {
        budget_name: fake::expected().budget_name,
        accounts: summary.accounts,
        transactions: summary.transactions,
        categories: summary.categories,
        first_date: first.to_string(),
        last_date: last.to_string(),
    }
}

#[tokio::test]
async fn the_summary_counts_match_the_builders() {
    let summary = summary_of(&fixtures().join(fake::EXPORT)).await.unwrap();
    assert_eq!(summary.budget_name, "Example Household");
    assert_eq!(as_expected(&summary), fake::expected());
}

#[tokio::test]
async fn a_shared_folder_and_an_older_export_read_the_same() {
    for file in [fake::IN_A_FOLDER, fake::OLDER] {
        let summary = summary_of(&fixtures().join(file)).await.unwrap();
        assert_eq!(as_expected(&summary), fake::expected(), "{file}");
    }
}

async fn read(zip: &Path) -> actual::Export {
    let dir = tempfile::tempdir().unwrap();
    unpack(zip, dir.path()).unwrap();
    let mut export = Export::open(dir.path()).await.unwrap();
    let read = export.read().await.unwrap();
    export.close().await;
    read
}

#[tokio::test]
async fn each_account_has_the_builders_rows_and_balance() {
    let export = read(&fixtures().join(fake::EXPORT)).await;
    let figures = |name: &str, off_budget, closed, bank_id: Option<&str>, rows, balance| {
        (
            name.to_owned(),
            off_budget,
            closed,
            bank_id.map(str::to_owned),
            rows,
            balance,
        )
    };
    let read: Vec<_> = export
        .accounts
        .iter()
        .map(|a| {
            figures(
                &a.name,
                a.off_budget,
                a.closed,
                a.bank_id.as_deref(),
                a.rows,
                a.balance.minor(),
            )
        })
        .collect();
    let expected: Vec<_> = fake::expected_accounts()
        .iter()
        .map(|e| figures(e.name, e.off_budget, e.closed, e.bank_id, e.rows, e.balance))
        .collect();
    assert_eq!(read, expected);
    // And by hand, so the builder's sums are checked too.
    assert_eq!(
        read,
        [
            figures(
                "Everyday checking",
                false,
                false,
                Some("fake-bank-0001"),
                43,
                2_145_801
            ),
            figures(
                "Rewards card",
                false,
                false,
                Some("fake-bank-0002"),
                30,
                -136_335
            ),
            figures("Rainy day savings", false, false, None, 2, 400_000),
            figures("Brokerage", true, false, None, 2, 1_100_000),
            figures("Old checking", false, true, None, 3, 0),
        ]
    );
}

#[tokio::test]
async fn every_case_in_the_fake_export_reads_as_actual_means_it() {
    let export = read(&fixtures().join(fake::EXPORT)).await;
    let category = |name: &str| {
        export
            .categories
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.id.clone())
    };
    let account = |name: &str| {
        export
            .accounts
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.id.clone())
    };
    let named = |name: &str| Some(Payee::Named(name.to_owned()));
    let to = |name: &str| account(name).map(Payee::Transfer);
    let find = |test: &dyn Fn(&actual::Transaction) -> bool| {
        let found: Vec<&actual::Transaction> =
            export.transactions.iter().filter(|t| test(t)).collect();
        assert_eq!(found.len(), 1, "{found:#?}");
        found[0]
    };
    let none = |test: &dyn Fn(&actual::Transaction) -> bool| {
        assert!(!export.transactions.iter().any(test));
    };

    // Tombstones: the deleted account, its row, the deleted row, and the deleted split are gone.
    assert_eq!(account("Deleted wallet"), None);
    none(&|t| t.amount.minor() == -700);
    none(&|t| t.amount.minor() == -5_000);
    none(&|t| t.amount.minor() == -6_000);
    // Deleted categories aren't listed; a group's deletion takes its categories with it.
    for gone in ["Restaurants", "Magazines", "Hobbies"] {
        assert_eq!(category(gone), None, "{gone}");
    }

    // Id indirection: a merged payee in a merged category.
    let dinner = find(&|t| t.notes.as_deref() == Some("Birthday dinner #gift #family"));
    assert_eq!(dinner.payee, named("Corner Grocer"));
    assert_eq!(dinner.category, category("Dining out"));
    // A category deleted without a merge, and a deleted payee: neither comes through.
    let magazines = find(&|t| t.amount.minor() == -1_999);
    assert_eq!(
        (magazines.payee.clone(), magazines.category.clone()),
        (named("Main Street Diner"), None)
    );
    let video = find(&|t| t.amount.minor() == -1_200);
    assert_eq!((video.payee.clone(), video.category.clone()), (None, None));

    // Formats: a NULL amount is 0; dates are days.
    let null = find(&|t| t.date.to_string() == "2025-11-11");
    assert_eq!(null.amount.minor(), 0);
    assert_eq!(null.category, category("Fuel"));

    // Bank text, the bank's id and notes.
    let mystery = find(&|t| t.amount.minor() == -999);
    assert_eq!(
        mystery.imported_description.as_deref(),
        Some("MYSTERY CHARGE 123")
    );
    assert_eq!(mystery.financial_id.as_deref(), Some("fake-fit-0999"));
    assert_eq!(
        (mystery.payee.clone(), mystery.category.clone()),
        (None, None)
    );

    // Transfers: the transfer payee is the other account, and the twins point at each other.
    for (from, into, amount) in [
        ("Everyday checking", "Rewards card", 50_000),
        ("Rainy day savings", "Brokerage", 100_000),
        ("Old checking", "Everyday checking", 29_000),
    ] {
        let out = find(&|t| t.amount.minor() == -amount && t.payee == to(into));
        let back = find(&|t| t.amount.minor() == amount && t.payee == to(from));
        assert_eq!(Some(&out.account), account(from).as_ref());
        assert_eq!(out.twin.as_ref(), Some(&back.id));
        assert_eq!(back.twin.as_ref(), Some(&out.id));
        assert_eq!((out.category.clone(), back.category.clone()), (None, None));
    }

    // Splits: the parent holds the total; its deleted line is gone.
    let split = find(&|t| t.notes.as_deref() == Some("Big shop #household"));
    assert_eq!(split.amount.minor(), -12_000);
    assert_eq!(split.category, None);
    let lines: Vec<(i64, Option<String>)> = split
        .lines
        .iter()
        .map(|line| (line.amount.minor(), line.category.clone()))
        .collect();
    assert_eq!(
        lines,
        [
            (-8_000, category("Groceries")),
            (-4_000, category("Dining out"))
        ]
    );

    // Starting balances: one per account, in the Starting Balances category.
    let starts: Vec<&actual::Transaction> = export
        .transactions
        .iter()
        .filter(|t| t.starting_balance)
        .collect();
    assert_eq!(starts.len(), 5);
    assert!(
        starts
            .iter()
            .all(|t| t.category == category("Starting Balances"))
    );
}

/// A category's group and name, whether it's Starting Balances, and its rows per account.
type Use<'a> = (&'a str, &'a str, bool, Vec<(&'a str, u64)>);

#[tokio::test]
async fn each_category_counts_the_imported_rows_using_it_per_account() {
    let export = read(&fixtures().join(fake::EXPORT)).await;
    let name = |id: &str| {
        export
            .accounts
            .iter()
            .find(|a| a.id == id)
            .map_or("?", |a| a.name.as_str())
    };
    let uses: Vec<Use> = export
        .categories
        .iter()
        .map(|c| {
            (
                c.group.as_str(),
                c.name.as_str(),
                c.is_starting_balances(),
                c.rows.iter().map(|(id, n)| (name(id), *n)).collect(),
            )
        })
        .collect();
    assert_eq!(
        uses,
        [
            (
                "Everyday",
                "Groceries",
                false,
                vec![("Rewards card", 14), ("Old checking", 1)]
            ),
            ("Everyday", "Dining out", false, vec![("Rewards card", 2)]),
            (
                "Everyday",
                "Fuel",
                false,
                vec![("Everyday checking", 1), ("Rewards card", 12)]
            ),
            ("Home", "Rent", false, vec![("Everyday checking", 12)]),
            ("Home", "Utilities", false, vec![("Everyday checking", 12)]),
            ("Income", "Paycheck", false, vec![("Everyday checking", 12)]),
            ("Income", "Starting Balances", true, vec![]),
        ]
    );
}

#[tokio::test]
async fn an_older_export_reads_the_same_but_for_split_line_ids() {
    let current = read(&fixtures().join(fake::EXPORT)).await;
    let mut older = read(&fixtures().join(fake::OLDER)).await;
    for transaction in &mut older.transactions {
        for line in &mut transaction.lines {
            let (parent, child) = line.id.split_once('/').unwrap();
            assert_eq!(parent, transaction.id);
            line.id = child.to_owned();
        }
    }
    assert_eq!(older, current);
}

#[tokio::test]
async fn a_zip_that_isnt_an_export_is_refused() {
    let error = summary_of(&fixtures().join(fake::NOT_ACTUAL))
        .await
        .unwrap_err();
    assert!(matches!(error, ReadError::NotAnExport), "{error}");
}

/// What a fixture holds, independent of how the zip and SQLite encode it: each entry's name, and
/// its text, or for a database every table's SQL and rows. So a new zlib or SQLite version
/// doesn't count as drift.
async fn contents(zip: &Path) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let mut archive = zip::ZipArchive::new(std::fs::File::open(zip).unwrap()).unwrap();
    let mut lines = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let name = entry.name().unwrap().into_owned();
        lines.push(format!("entry {name}"));
        if !name.ends_with(".sqlite") {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut entry, &mut text).unwrap();
            lines.push(text);
            continue;
        }
        let path = dir.path().join(format!("{i}.sqlite"));
        std::io::copy(&mut entry, &mut std::fs::File::create(&path).unwrap()).unwrap();
        let mut db = SqliteConnectOptions::new()
            .filename(&path)
            .read_only(true)
            .connect()
            .await
            .unwrap();
        let schema: Vec<(String, String, Option<String>)> =
            sqlx::query_as("SELECT type, name, sql FROM sqlite_schema ORDER BY name")
                .fetch_all(&mut db)
                .await
                .unwrap();
        for (kind, table, sql) in schema {
            lines.push(format!("{kind} {table}: {}", sql.unwrap_or_default()));
            if kind == "table" {
                let columns: Vec<String> =
                    sqlx::query_scalar("SELECT format('\"%w\"', name) FROM pragma_table_info(?)")
                        .bind(&table)
                        .fetch_all(&mut db)
                        .await
                        .unwrap();
                let rows: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                    "SELECT json_array({}) FROM \"{table}\" ORDER BY rowid",
                    columns.join(", ")
                )))
                .fetch_all(&mut db)
                .await
                .unwrap();
                lines.extend(rows);
            }
        }
        db.close().await.unwrap();
    }
    lines
}

#[tokio::test]
async fn the_committed_fixtures_match_the_builder() {
    let fresh = tempfile::tempdir().unwrap();
    fake::write_all(fresh.path()).await.unwrap();
    for file in fake::FILES {
        let committed = fixtures().join(file);
        assert!(
            committed.exists(),
            "fixtures/{file} is missing: run `just fixtures` and commit the result"
        );
        assert!(
            contents(&committed).await == contents(&fresh.path().join(file)).await,
            "fixtures/{file} doesn't match the builder: run `just fixtures` and commit the result"
        );
    }
}
