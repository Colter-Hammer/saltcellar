//! The committed fake exports in `fixtures/`: what the reader makes of them, and that they still
//! match what the builder ([`crate::fake`]) writes.

use std::path::{Path, PathBuf};

use crate::{Export, ReadError, Summary, fake, unpack};
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
