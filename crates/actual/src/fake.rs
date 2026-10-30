//! The fake Actual export: a made-up household, written as the zips in `fixtures/`
//! (docs/spec/foundations.md#test-data). Everything in it is invented.
//!
//! `just fixtures` writes the zips (`examples/fixtures.rs`); the tests in `tests.rs` fail when the
//! committed ones no longer match what this writes, and checks the reader's counts against
//! [`expected`].
//!
//! The household covers every case the importer handles: tombstoned accounts, categories, groups,
//! payees and transactions; merged payees and categories; a split with a tombstoned child; linked
//! transfer pairs, one to an account meant to be skipped; starting balances; a closed account; an
//! off-budget account; notes with `#tags`; uncategorized rows; a NULL amount; a category that
//! resolves to a tombstoned one; a tombstoned split parent; a row in a tombstoned account; and a
//! stale view that the reader must not use.

use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
use sqlx::{ConnectOptions, Connection};
use zip::write::SimpleFileOptions;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// The fake export, `db.sqlite` and `metadata.json` at the zip's root.
pub const EXPORT: &str = "actual-export.zip";
/// The same export with both files inside one folder.
pub const IN_A_FOLDER: &str = "actual-export-in-a-folder.zip";
/// The same household from an older Actual, without `transactions.parent_id`: split children's
/// ids are `<parent id>/<child id>` instead *(agent)*.
pub const OLDER: &str = "actual-export-older.zip";
/// A zip that isn't an Actual export.
pub const NOT_ACTUAL: &str = "not-an-actual-export.zip";

/// Every file [`write_all`] writes.
pub const FILES: &[&str] = &[EXPORT, IN_A_FOLDER, OLDER, NOT_ACTUAL];

const BUDGET_NAME: &str = "Example Household";
const BUDGET_ID: &str = "Example-Household-0a1b2c3";

/// What the importer's step 1 should report for each export, worked out from the household here
/// rather than by the reader.
#[derive(Debug, PartialEq, Eq)]
pub struct Expected {
    pub budget_name: &'static str,
    pub accounts: u64,
    pub transactions: u64,
    pub categories: u64,
    pub first_date: String,
    pub last_date: String,
}

/// The counts the importer should show for the fake export (tombstones excluded).
#[must_use]
pub fn expected() -> Expected {
    let household = household();
    let live_account = |id: &str| {
        household
            .accounts
            .iter()
            .any(|a| a.id == id && !a.tombstone)
    };
    let row = |id: &str| household.transactions.iter().find(|t| t.id == id);
    let alive: Vec<&Transaction> = household
        .transactions
        .iter()
        .filter(|t| !t.tombstone && live_account(&t.account))
        .filter(|t| match &t.parent {
            Some(parent) => row(parent).is_some_and(|p| !p.tombstone),
            None => true,
        })
        .collect();
    let dates: Vec<u32> = alive.iter().map(|t| t.date).collect();
    let show = |date: u32| {
        format!(
            "{}-{:02}-{:02}",
            date / 10_000,
            date / 100 % 100,
            date % 100
        )
    };
    Expected {
        budget_name: BUDGET_NAME,
        accounts: household.accounts.iter().filter(|a| !a.tombstone).count() as u64,
        transactions: alive.iter().filter(|t| t.parent.is_none()).count() as u64,
        categories: household.categories.iter().filter(|c| !c.tombstone).count() as u64,
        first_date: show(dates.iter().copied().min().unwrap_or_default()),
        last_date: show(dates.iter().copied().max().unwrap_or_default()),
    }
}

/// One live account as step 2 of the importer should show it, worked out from the household here
/// rather than by the reader.
#[derive(Debug, PartialEq, Eq)]
pub struct ExpectedAccount {
    pub name: &'static str,
    pub off_budget: bool,
    pub closed: bool,
    pub bank_id: Option<&'static str>,
    /// A split counts once; starting balances count.
    pub rows: u64,
    /// Cents: every alive row's amount, a split's total once.
    pub balance: i64,
}

/// The live accounts in the order the reader puts them: open before closed, on budget before off
/// budget, then Actual's order.
#[must_use]
pub fn expected_accounts() -> Vec<ExpectedAccount> {
    let household = household();
    let mut accounts: Vec<&Account> = household.accounts.iter().filter(|a| !a.tombstone).collect();
    accounts.sort_by_key(|a| (a.closed, a.off_budget));
    accounts
        .into_iter()
        .map(|account| {
            let rows: Vec<&Transaction> = household
                .transactions
                .iter()
                .filter(|t| t.account == account.id && !t.tombstone && t.parent.is_none())
                .collect();
            ExpectedAccount {
                name: account.name,
                off_budget: account.off_budget,
                closed: account.closed,
                bank_id: account.bank_id,
                rows: rows.len() as u64,
                balance: rows.iter().map(|t| t.amount.unwrap_or(0)).sum(),
            }
        })
        .collect()
}

/// Writes every file in [`FILES`] into `dir`.
///
/// # Errors
/// If a file can't be written.
pub async fn write_all(dir: &Path) -> Result<()> {
    let household = household();
    let scratch = tempfile::tempdir()?;
    let metadata = serde_json::to_vec_pretty(&serde_json::json!({
        "id": BUDGET_ID,
        "budgetName": BUDGET_NAME,
        "resetClock": true,
    }))?;

    let db = scratch.path().join("db.sqlite");
    household.write(&db, Schema::Current).await?;
    let db = std::fs::read(db)?;
    write_zip(
        &dir.join(EXPORT),
        &[("db.sqlite", &db), ("metadata.json", &metadata)],
        None,
    )?;
    let folder = format!("{BUDGET_ID}/");
    write_zip(
        &dir.join(IN_A_FOLDER),
        &[
            (&format!("{folder}db.sqlite"), &db),
            (&format!("{folder}metadata.json"), &metadata),
        ],
        Some(&folder),
    )?;

    let older = scratch.path().join("older.sqlite");
    household.write(&older, Schema::Older).await?;
    write_zip(
        &dir.join(OLDER),
        &[
            ("db.sqlite", &std::fs::read(older)?),
            ("metadata.json", &metadata),
        ],
        None,
    )?;

    write_zip(
        &dir.join(NOT_ACTUAL),
        &[
            ("readme.txt", b"Holiday photos, sorted by year.\n"),
            ("photos.json", b"{\"albums\": [\"Beach\", \"Mountains\"]}\n"),
        ],
        None,
    )?;
    Ok(())
}

/// A zip of `entries`, with fixed times and modes so the bytes only change with the content.
fn write_zip(path: &Path, entries: &[(&str, &[u8])], folder: Option<&str>) -> Result<()> {
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::from_date_and_time(2026, 1, 1, 0, 0, 0)?)
        .unix_permissions(0o644);
    let mut zip = zip::ZipWriter::new(File::create(path)?);
    if let Some(folder) = folder {
        zip.add_directory(folder, options.unix_permissions(0o755))?;
    }
    for (name, bytes) in entries {
        zip.start_file(*name, options)?;
        zip.write_all(bytes)?;
    }
    zip.finish()?;
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Schema {
    Current,
    /// No `transactions.parent_id`.
    Older,
}

struct Household {
    accounts: Vec<Account>,
    groups: Vec<Group>,
    categories: Vec<Category>,
    /// `(id, transferId)`: a merged category points at the one it went into.
    category_mapping: Vec<(String, String)>,
    payees: Vec<Payee>,
    /// `(id, targetId)`: a merged payee points at the one it went into.
    payee_mapping: Vec<(String, String)>,
    transactions: Vec<Transaction>,
}

struct Account {
    id: String,
    name: &'static str,
    off_budget: bool,
    closed: bool,
    bank_id: Option<&'static str>,
    tombstone: bool,
}

struct Group {
    id: String,
    name: &'static str,
    income: bool,
    tombstone: bool,
}

struct Category {
    id: String,
    name: &'static str,
    group: String,
    income: bool,
    tombstone: bool,
}

struct Payee {
    id: String,
    name: &'static str,
    transfer_account: Option<String>,
    tombstone: bool,
}

#[derive(Default)]
struct Transaction {
    id: String,
    account: String,
    category: Option<String>,
    amount: Option<i64>,
    payee: Option<String>,
    imported: Option<String>,
    notes: Option<&'static str>,
    /// `YYYYMMDD`.
    date: u32,
    financial_id: Option<String>,
    is_parent: bool,
    parent: Option<String>,
    transferred_id: Option<String>,
    starting_balance: bool,
    tombstone: bool,
}

/// Actual's `YYYYMMDD` date.
fn ymd(year: u32, month: u32, day: u32) -> u32 {
    year * 10_000 + month * 100 + day
}

/// A UUID-shaped id, as Actual's are: `kind` tells the tables apart.
fn id(kind: u32, n: u32) -> String {
    format!("{kind:08x}-0000-4000-8000-{n:012x}")
}

fn a(n: u32) -> String {
    id(1, n)
}

fn c(n: u32) -> String {
    id(3, n)
}

fn p(n: u32) -> String {
    id(4, n)
}

/// The transfer payee of account `n`: money moved to or from it.
fn to(n: u32) -> String {
    id(5, n)
}

/// The bank's text and transaction id for a row that came from bank sync.
fn bank(text: &str, n: u32) -> (Option<String>, Option<String>) {
    (Some(text.to_owned()), Some(format!("fake-fit-{n:04}")))
}

fn household() -> Household {
    let accounts = accounts();
    let (groups, categories, category_mapping) = categories();
    let (payees, payee_mapping) = payees(&accounts);
    let mut rows = Rows::default();
    starting_balances(&mut rows);
    everyday(&mut rows);
    transfers(&mut rows);
    splits(&mut rows);
    odd_rows(&mut rows);
    Household {
        accounts,
        groups,
        categories,
        category_mapping,
        payees,
        payee_mapping,
        transactions: rows.list,
    }
}

fn accounts() -> Vec<Account> {
    let account = |n, name, off_budget, closed, bank_id, tombstone| Account {
        id: id(1, n),
        name,
        off_budget,
        closed,
        bank_id,
        tombstone,
    };
    vec![
        account(
            1,
            "Everyday checking",
            false,
            false,
            Some("fake-bank-0001"),
            false,
        ),
        account(
            2,
            "Rewards card",
            false,
            false,
            Some("fake-bank-0002"),
            false,
        ),
        account(3, "Rainy day savings", false, false, None, false),
        // Off budget, and the account meant to be skipped in step 2.
        account(4, "Brokerage", true, false, None, false),
        account(5, "Old checking", false, true, None, false),
        account(6, "Deleted wallet", false, false, None, true),
    ]
}

type Mapping = Vec<(String, String)>;

fn categories() -> (Vec<Group>, Vec<Category>, Mapping) {
    let group = |n, name, income, tombstone| Group {
        id: id(2, n),
        name,
        income,
        tombstone,
    };
    let groups = vec![
        group(1, "Everyday", false, false),
        group(2, "Home", false, false),
        group(3, "Income", true, false),
        group(4, "Old stuff", false, true),
    ];

    let category = |n, name, group, tombstone| Category {
        id: id(3, n),
        name,
        group: id(2, group),
        income: group == 3,
        tombstone,
    };
    let categories = vec![
        category(1, "Groceries", 1, false),
        category(2, "Dining out", 1, false),
        category(3, "Fuel", 1, false),
        category(4, "Rent", 2, false),
        category(5, "Utilities", 2, false),
        category(6, "Paycheck", 3, false),
        category(7, "Starting Balances", 3, false),
        // Merged into Dining out.
        category(8, "Restaurants", 1, true),
        // Deleted without a transfer: rows in it are uncategorized.
        category(9, "Magazines", 1, true),
        category(10, "Hobbies", 4, true),
    ];
    let mut category_mapping: Vec<(String, String)> = categories
        .iter()
        .map(|category| (category.id.clone(), category.id.clone()))
        .collect();
    remap(&mut category_mapping, &id(3, 8), &id(3, 2));

    (groups, categories, category_mapping)
}

fn payees(accounts: &[Account]) -> (Vec<Payee>, Mapping) {
    let payee = |n, name, tombstone| Payee {
        id: id(4, n),
        name,
        transfer_account: None,
        tombstone,
    };
    let mut payees = vec![
        payee(1, "Corner Grocer", false),
        payee(2, "Fuel Stop", false),
        payee(3, "Main Street Diner", false),
        payee(4, "Acme Payroll", false),
        payee(5, "Landlord Properties", false),
        payee(6, "City Power and Water", false),
        // Merged into Corner Grocer.
        payee(7, "Corner Grocery 12", true),
        payee(8, "Old Video Store", true),
    ];
    // Every account has a transfer payee, named '' (Actual shows the account's name).
    for (n, account) in (1..).zip(accounts) {
        payees.push(Payee {
            id: id(5, n),
            name: "",
            transfer_account: Some(account.id.clone()),
            tombstone: account.tombstone,
        });
    }
    let mut payee_mapping: Vec<(String, String)> = payees
        .iter()
        .map(|payee| (payee.id.clone(), payee.id.clone()))
        .collect();
    remap(&mut payee_mapping, &id(4, 7), &id(4, 1));

    (payees, payee_mapping)
}

/// The transactions, each given the next id as it's added.
#[derive(Default)]
struct Rows {
    list: Vec<Transaction>,
}

impl Rows {
    fn add(&mut self, transaction: Transaction) -> String {
        let id = id(6, u32::try_from(self.list.len()).unwrap_or(u32::MAX) + 1);
        self.list.push(Transaction {
            id: id.clone(),
            ..transaction
        });
        id
    }

    /// Links two transfer legs through `transferred_id`.
    fn link(&mut self, one: &str, other: &str) {
        for transaction in &mut self.list {
            if transaction.id == one {
                transaction.transferred_id = Some(other.to_owned());
            } else if transaction.id == other {
                transaction.transferred_id = Some(one.to_owned());
            }
        }
    }
}

fn starting_balances(rows: &mut Rows) {
    // Starting balances.
    for (account, amount) in [
        (1, 250_000),
        (2, -12_000),
        (3, 500_000),
        (4, 1_000_000),
        (5, 30_000),
    ] {
        rows.add(Transaction {
            account: a(account),
            category: Some(c(7)),
            amount: Some(amount),
            date: ymd(2025, 1, 1),
            starting_balance: true,
            ..Transaction::default()
        });
    }
}

fn everyday(rows: &mut Rows) {
    // A year of everyday life.
    for month in 1..=12u32 {
        let day = |d: u32| ymd(2025, month, d);
        let fit = month * 10;
        let (imported, financial_id) = bank("ACME PAYROLL DIRECT DEP", fit + 1);
        rows.add(Transaction {
            account: a(1),
            category: Some(c(6)),
            amount: Some(320_000),
            payee: Some(p(4)),
            imported,
            financial_id,
            date: day(1),
            ..Transaction::default()
        });
        rows.add(Transaction {
            account: a(1),
            category: Some(c(4)),
            amount: Some(-150_000),
            payee: Some(p(5)),
            date: day(3),
            ..Transaction::default()
        });
        let (imported, financial_id) = bank("CITY POWER WATER AUTOPAY", fit + 2);
        rows.add(Transaction {
            account: a(1),
            category: Some(c(5)),
            amount: Some(-(9_000 + i64::from(month) * 100)),
            payee: Some(p(6)),
            imported,
            financial_id,
            notes: Some("Monthly bill #bills"),
            date: day(15),
            ..Transaction::default()
        });
        let (imported, financial_id) = bank("CORNER GROCER 0042", fit + 3);
        rows.add(Transaction {
            account: a(2),
            category: Some(c(1)),
            amount: Some(-(8_000 + i64::from(month) * 37)),
            payee: Some(p(1)),
            imported,
            financial_id,
            date: day(8),
            ..Transaction::default()
        });
        let (imported, financial_id) = bank("FUEL STOP 77", fit + 4);
        rows.add(Transaction {
            account: a(2),
            category: Some(c(3)),
            amount: Some(-4_500),
            payee: Some(p(2)),
            imported,
            financial_id,
            date: day(20),
            ..Transaction::default()
        });
    }
}

fn transfers(rows: &mut Rows) {
    // A card payment: a linked transfer pair between two budgeted accounts, no category.
    let out = rows.add(Transaction {
        account: a(1),
        amount: Some(-50_000),
        payee: Some(to(2)),
        date: ymd(2025, 6, 25),
        ..Transaction::default()
    });
    let into = rows.add(Transaction {
        account: a(2),
        amount: Some(50_000),
        payee: Some(to(1)),
        date: ymd(2025, 6, 25),
        ..Transaction::default()
    });
    rows.link(&out, &into);

    // A linked pair whose other account (Brokerage) is meant to be skipped.
    let out = rows.add(Transaction {
        account: a(3),
        amount: Some(-100_000),
        payee: Some(to(4)),
        date: ymd(2025, 9, 10),
        notes: Some("Yearly top-up #investing"),
        ..Transaction::default()
    });
    let into = rows.add(Transaction {
        account: a(4),
        amount: Some(100_000),
        payee: Some(to(3)),
        date: ymd(2025, 9, 10),
        ..Transaction::default()
    });
    rows.link(&out, &into);

    // The closed account: a row, then its balance moved out before closing.
    rows.add(Transaction {
        account: a(5),
        category: Some(c(1)),
        amount: Some(-1_000),
        payee: Some(p(1)),
        date: ymd(2025, 1, 15),
        ..Transaction::default()
    });
    let out = rows.add(Transaction {
        account: a(5),
        amount: Some(-29_000),
        payee: Some(to(1)),
        notes: Some("Closing account"),
        date: ymd(2025, 3, 1),
        ..Transaction::default()
    });
    let into = rows.add(Transaction {
        account: a(1),
        amount: Some(29_000),
        payee: Some(to(5)),
        date: ymd(2025, 3, 1),
        ..Transaction::default()
    });
    rows.link(&out, &into);
}

fn splits(rows: &mut Rows) {
    // A split: the parent holds the total, the children the categories; one child was deleted.
    let parent = rows.add(Transaction {
        account: a(2),
        amount: Some(-12_000),
        payee: Some(p(1)),
        notes: Some("Big shop #household"),
        date: ymd(2025, 7, 12),
        is_parent: true,
        ..Transaction::default()
    });
    for (category, amount, tombstone) in [(1, -8_000, false), (2, -4_000, false), (3, -1_500, true)]
    {
        rows.add(Transaction {
            account: a(2),
            category: Some(c(category)),
            amount: Some(amount),
            payee: Some(p(1)),
            date: ymd(2025, 7, 12),
            parent: Some(parent.clone()),
            tombstone,
            ..Transaction::default()
        });
    }

    // A deleted split: its children aren't tombstoned themselves, but aren't alive either.
    let parent = rows.add(Transaction {
        account: a(1),
        amount: Some(-6_000),
        payee: Some(p(3)),
        date: ymd(2025, 12, 20),
        is_parent: true,
        tombstone: true,
        ..Transaction::default()
    });
    for amount in [-2_500, -3_500] {
        rows.add(Transaction {
            account: a(1),
            category: Some(c(2)),
            amount: Some(amount),
            payee: Some(p(3)),
            date: ymd(2025, 12, 20),
            parent: Some(parent.clone()),
            ..Transaction::default()
        });
    }
}

fn odd_rows(rows: &mut Rows) {
    // A merged payee in a merged category: Corner Grocer, Dining out.
    rows.add(Transaction {
        account: a(2),
        category: Some(c(8)),
        amount: Some(-2_300),
        payee: Some(p(7)),
        notes: Some("Birthday dinner #gift #family"),
        date: ymd(2025, 4, 5),
        ..Transaction::default()
    });
    // A category deleted without a transfer: uncategorized.
    rows.add(Transaction {
        account: a(1),
        category: Some(c(9)),
        amount: Some(-1_999),
        payee: Some(p(3)),
        date: ymd(2025, 5, 9),
        ..Transaction::default()
    });
    // Uncategorized in Actual, one without a payee.
    rows.add(Transaction {
        account: a(1),
        amount: Some(-4_200),
        payee: Some(p(3)),
        imported: Some("MAIN ST DINER".to_owned()),
        date: ymd(2025, 8, 14),
        ..Transaction::default()
    });
    rows.add(Transaction {
        account: a(2),
        amount: Some(-999),
        imported: Some("MYSTERY CHARGE 123".to_owned()),
        financial_id: Some("fake-fit-0999".to_owned()),
        date: ymd(2025, 10, 2),
        ..Transaction::default()
    });
    // A NULL amount, which reads as 0.
    rows.add(Transaction {
        account: a(1),
        category: Some(c(3)),
        payee: Some(p(2)),
        date: ymd(2025, 11, 11),
        ..Transaction::default()
    });
    // A deleted row, a row from a deleted payee, and a row in the deleted account.
    rows.add(Transaction {
        account: a(1),
        category: Some(c(1)),
        amount: Some(-5_000),
        payee: Some(p(1)),
        date: ymd(2025, 2, 2),
        tombstone: true,
        ..Transaction::default()
    });
    rows.add(Transaction {
        account: a(1),
        category: Some(c(10)),
        amount: Some(-1_200),
        payee: Some(p(8)),
        date: ymd(2025, 2, 18),
        ..Transaction::default()
    });
    rows.add(Transaction {
        account: a(6),
        amount: Some(-700),
        date: ymd(2025, 3, 3),
        ..Transaction::default()
    });
    // The newest row, into the next year.
    rows.add(Transaction {
        account: a(2),
        category: Some(c(1)),
        amount: Some(-6_150),
        payee: Some(p(1)),
        date: ymd(2026, 1, 5),
        ..Transaction::default()
    });
}

/// Points `from`'s mapping at `into`, as merging does in Actual.
fn remap(mapping: &mut [(String, String)], from: &str, into: &str) {
    for (id, target) in mapping.iter_mut() {
        if id == from {
            into.clone_into(target);
        }
    }
}

impl Household {
    /// Writes Actual's raw tables (the subset the importer reads) into a new SQLite file.
    async fn write(&self, path: &Path, schema: Schema) -> Result<()> {
        let mut db = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .connect()
            .await?;
        let parent_id = if schema == Schema::Current {
            ", parent_id TEXT"
        } else {
            ""
        };
        for ddl in [
            "CREATE TABLE accounts (id TEXT PRIMARY KEY, account_id TEXT, name TEXT, \
             balance_current INTEGER, balance_available INTEGER, balance_limit INTEGER, mask TEXT, \
             official_name TEXT, subtype TEXT, bank TEXT, offbudget INTEGER DEFAULT 0, \
             closed INTEGER DEFAULT 0, tombstone INTEGER DEFAULT 0, sort_order REAL, type TEXT)"
                .to_owned(),
            "CREATE TABLE category_groups (id TEXT PRIMARY KEY, name TEXT, \
             is_income INTEGER DEFAULT 0, sort_order REAL, tombstone INTEGER DEFAULT 0, \
             hidden BOOLEAN NOT NULL DEFAULT 0)"
                .to_owned(),
            "CREATE TABLE categories (id TEXT PRIMARY KEY, name TEXT, is_income INTEGER DEFAULT 0, \
             cat_group TEXT, sort_order REAL, tombstone INTEGER DEFAULT 0, \
             hidden BOOLEAN NOT NULL DEFAULT 0)"
                .to_owned(),
            "CREATE TABLE category_mapping (id TEXT PRIMARY KEY, transferId TEXT)".to_owned(),
            "CREATE TABLE payees (id TEXT PRIMARY KEY, name TEXT, category TEXT, \
             tombstone INTEGER DEFAULT 0, transfer_acct TEXT)"
                .to_owned(),
            "CREATE TABLE payee_mapping (id TEXT PRIMARY KEY, targetId TEXT)".to_owned(),
            format!(
                "CREATE TABLE transactions (id TEXT PRIMARY KEY, isParent INTEGER DEFAULT 0, \
                 isChild INTEGER DEFAULT 0, acct TEXT, category TEXT, amount INTEGER, \
                 description TEXT, notes TEXT, date INTEGER, financial_id TEXT, type TEXT, \
                 location TEXT, error TEXT, imported_description TEXT, \
                 starting_balance_flag INTEGER DEFAULT 0, transferred_id TEXT, sort_order REAL, \
                 tombstone INTEGER DEFAULT 0, cleared INTEGER DEFAULT 1, pending INTEGER DEFAULT 0, \
                 schedule TEXT, reconciled INTEGER DEFAULT 0{parent_id})"
            ),
            // A stale view, as an export last opened by an older Actual can have: reading it
            // instead of the raw tables would find nothing.
            "CREATE VIEW v_transactions AS SELECT * FROM transactions WHERE 0".to_owned(),
        ] {
            sqlx::query(sqlx::AssertSqlSafe(ddl)).execute(&mut db).await?;
        }

        self.insert(&mut db, schema).await?;
        sqlx::query("VACUUM").execute(&mut db).await?;
        db.close().await?;
        Ok(())
    }

    async fn insert(&self, db: &mut SqliteConnection, schema: Schema) -> Result<()> {
        for (i, account) in (0u32..).zip(&self.accounts) {
            sqlx::query(
                "INSERT INTO accounts (id, account_id, name, offbudget, closed, tombstone, sort_order) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&account.id)
            .bind(account.bank_id)
            .bind(account.name)
            .bind(account.off_budget)
            .bind(account.closed)
            .bind(account.tombstone)
            .bind(f64::from(i))
            .execute(&mut *db)
            .await?;
        }
        for (i, group) in (0u32..).zip(&self.groups) {
            sqlx::query(
                "INSERT INTO category_groups (id, name, is_income, sort_order, tombstone) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&group.id)
            .bind(group.name)
            .bind(group.income)
            .bind(f64::from(i))
            .bind(group.tombstone)
            .execute(&mut *db)
            .await?;
        }
        for (i, category) in (0u32..).zip(&self.categories) {
            sqlx::query(
                "INSERT INTO categories (id, name, is_income, cat_group, sort_order, tombstone) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&category.id)
            .bind(category.name)
            .bind(category.income)
            .bind(&category.group)
            .bind(f64::from(i))
            .bind(category.tombstone)
            .execute(&mut *db)
            .await?;
        }
        for (table, from, to, rows) in [
            (
                "category_mapping",
                "id",
                "transferId",
                &self.category_mapping,
            ),
            ("payee_mapping", "id", "targetId", &self.payee_mapping),
        ] {
            for (id, target) in rows {
                let sql = format!("INSERT INTO {table} ({from}, {to}) VALUES (?, ?)");
                sqlx::query(sqlx::AssertSqlSafe(sql))
                    .bind(id)
                    .bind(target)
                    .execute(&mut *db)
                    .await?;
            }
        }
        for payee in &self.payees {
            sqlx::query(
                "INSERT INTO payees (id, name, transfer_acct, tombstone) VALUES (?, ?, ?, ?)",
            )
            .bind(&payee.id)
            .bind(payee.name)
            .bind(&payee.transfer_account)
            .bind(payee.tombstone)
            .execute(&mut *db)
            .await?;
        }
        self.insert_transactions(db, schema).await
    }

    async fn insert_transactions(&self, db: &mut SqliteConnection, schema: Schema) -> Result<()> {
        for (i, t) in (0u32..).zip(&self.transactions) {
            // Older exports name a split child `<parent id>/<child id>` and have no parent_id.
            let id = match (&t.parent, schema) {
                (Some(parent), Schema::Older) => format!("{parent}/{}", t.id),
                _ => t.id.clone(),
            };
            let sql = format!(
                "INSERT INTO transactions (id, isParent, isChild, acct, category, amount, \
                 description, notes, date, financial_id, imported_description, \
                 starting_balance_flag, transferred_id, sort_order, tombstone{}) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?{})",
                if schema == Schema::Current {
                    ", parent_id"
                } else {
                    ""
                },
                if schema == Schema::Current { ", ?" } else { "" },
            );
            let mut insert = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(id)
                .bind(t.is_parent)
                .bind(t.parent.is_some())
                .bind(&t.account)
                .bind(&t.category)
                .bind(t.amount)
                .bind(&t.payee)
                .bind(t.notes)
                .bind(t.date)
                .bind(&t.financial_id)
                .bind(&t.imported)
                .bind(t.starting_balance)
                .bind(&t.transferred_id)
                .bind(f64::from(i))
                .bind(t.tombstone);
            if schema == Schema::Current {
                insert = insert.bind(&t.parent);
            }
            insert.execute(&mut *db).await?;
        }
        Ok(())
    }
}
