use std::collections::BTreeMap;

use super::*;
use crate::actual::{Account, Category, Line};

fn date(text: &str) -> Date {
    text.parse().unwrap()
}

fn account(id: &str, name: &str, balance: i64) -> Account {
    Account {
        id: id.to_owned(),
        name: name.to_owned(),
        off_budget: false,
        closed: false,
        bank_id: None,
        rows: 0,
        balance: Money::from_minor(balance),
    }
}

fn category(id: &str, name: &str) -> Category {
    Category {
        id: id.to_owned(),
        name: name.to_owned(),
        group: "Everyday".to_owned(),
        income: false,
        rows: BTreeMap::new(),
    }
}

fn row(id: &str, account: &str, day: &str, amount: i64) -> Transaction {
    Transaction {
        id: id.to_owned(),
        account: account.to_owned(),
        date: date(day),
        amount: Money::from_minor(amount),
        payee: None,
        category: None,
        imported_description: None,
        financial_id: None,
        notes: None,
        twin: None,
        starting_balance: false,
        lines: Vec::new(),
    }
}

fn named(name: &str) -> Payee {
    Payee::Named(name.to_owned())
}

fn create(name: &str) -> AccountChoice {
    AccountChoice::Create {
        name: name.to_owned(),
        kind: AccountType::Checking,
        role: Role::Budgeted,
        balances_only: false,
    }
}

/// Checking `a` and savings `b`, categories Groceries `c1`, Fuel `c2` and Rent `c3`.
fn export(transactions: Vec<Transaction>) -> Export {
    Export {
        accounts: vec![account("a", "Checking", 0), account("b", "Savings", 0)],
        categories: vec![
            category("c1", "Groceries"),
            category("c2", "Fuel"),
            category("c3", "Rent"),
        ],
        transactions,
    }
}

fn choices(accounts: &[(&str, AccountChoice)], categories: &[(&str, GoesTo)]) -> Choices {
    Choices {
        accounts: accounts
            .iter()
            .map(|(id, choice)| ((*id).to_owned(), choice.clone()))
            .collect(),
        categories: categories
            .iter()
            .map(|(id, goes_to)| ((*id).to_owned(), *goes_to))
            .collect(),
    }
}

fn both_created() -> Vec<(&'static str, AccountChoice)> {
    vec![("a", create("Checking")), ("b", create("Savings"))]
}

const APP_CATEGORY: i64 = 7;

fn plan(export: &Export, choices: &Choices) -> Result<Plan, ChoiceError> {
    let categories = HashSet::from([APP_CATEGORY]);
    let accounts = [(1, "Old wallet".to_owned())];
    Plan::new(
        export,
        choices,
        Existing {
            accounts: &accounts,
            categories: &categories,
        },
    )
}

fn rows(plan: &Plan) -> Vec<&Row> {
    plan.accounts.iter().flat_map(|a| &a.rows).collect()
}

#[test]
fn each_category_goes_where_its_choice_says() {
    let mut groceries = row("t1", "a", "2025-03-01", -1_250);
    groceries.category = Some("c1".to_owned());
    groceries.payee = Some(named("Corner Grocer"));
    groceries.imported_description = Some("CORNER GROCER #12, Springfield".to_owned());
    groceries.financial_id = Some("bank-1".to_owned());
    groceries.notes = Some("Weekly shop #food #home #food".to_owned());
    let mut fuel = row("t2", "a", "2025-03-02", -4_000);
    fuel.category = Some("c2".to_owned());
    fuel.payee = Some(named("Fill-Up"));
    let mut rent = row("t3", "a", "2025-03-03", -90_000);
    rent.category = Some("c3".to_owned());
    let none = row("t4", "a", "2025-03-04", -100);
    let mut transfer = row("t5", "a", "2025-03-05", -5_000);
    transfer.payee = Some(Payee::Transfer("b".to_owned()));
    transfer.category = Some("c1".to_owned());
    let export = export(vec![groceries, fuel, rent, none, transfer]);
    let choices = choices(
        &both_created(),
        &[
            ("c1", GoesTo::Category(APP_CATEGORY)),
            ("c2", GoesTo::Transfer),
        ],
    );
    let plan = plan(&export, &choices).unwrap();
    let rows = rows(&plan);

    assert_eq!(
        *rows[0],
        Row {
            import_ref: "t1".to_owned(),
            date: date("2025-03-01"),
            amount: Money::from_minor(-1_250),
            raw_description: "CORNER GROCER #12, Springfield".to_owned(),
            external_id: Some("bank-1".to_owned()),
            payee: Some("Corner Grocer".to_owned()),
            alias: Some("corner grocer 12 springfield".to_owned()),
            memo: Some("Weekly shop #food #home #food".to_owned()),
            tags: vec!["food".to_owned(), "home".to_owned()],
            disposition: Disposition::Mapped {
                category_id: APP_CATEGORY,
                reason: "From Actual: Groceries".to_owned(),
            },
            source_category: Some("Groceries".to_owned()),
        }
    );
    // Without the bank's text, the payee's name is the description.
    assert_eq!(rows[1].raw_description, "Fill-Up");
    assert_eq!(rows[1].alias.as_deref(), Some("fillup"));
    assert_eq!(rows[1].disposition, Disposition::Transfer);
    // Rent has no choice: Decide later.
    assert_eq!(rows[2].disposition, Disposition::Undecided);
    assert_eq!(rows[2].source_category.as_deref(), Some("Rent"));
    assert_eq!(rows[2].raw_description, "");
    assert_eq!(rows[2].alias, None);
    assert_eq!(rows[3].disposition, Disposition::Uncategorized);
    assert_eq!(rows[3].source_category, None);
    // A transfer leg is transfer-like whatever its category, and has no payee.
    assert_eq!(rows[4].disposition, Disposition::Transfer);
    assert_eq!(rows[4].payee, None);
    assert_eq!(rows[4].raw_description, "Savings");
    assert_eq!(rows[4].source_category.as_deref(), Some("Groceries"));

    assert_eq!(
        plan.counts(),
        Counts {
            mapped: 1,
            undecided: 1,
            transfer: 2,
            uncategorized: 1,
        }
    );
}

#[test]
fn a_split_comes_in_unsplit_with_the_split_in_its_memo() {
    let mut split = row("s", "a", "2025-03-01", -3_005);
    split.payee = Some(named("Big Box"));
    split.notes = Some("Big shop #household".to_owned());
    let line = |id: &str, amount, category: Option<&str>, notes: Option<&str>| Line {
        id: id.to_owned(),
        amount: Money::from_minor(amount),
        payee: None,
        category: category.map(str::to_owned),
        notes: notes.map(str::to_owned),
        twin: None,
    };
    split.lines = vec![
        line("s1", -2_000, Some("c1"), None),
        line("s2", -1_005, None, Some("gift #birthday")),
        line("s3", 0, Some("c2"), Some("  ")),
    ];
    let export = export(vec![split]);
    let plan = plan(
        &export,
        &choices(&both_created(), &[("c1", GoesTo::Category(APP_CATEGORY))]),
    )
    .unwrap();
    let row = rows(&plan)[0];
    assert_eq!(row.amount, Money::from_minor(-3_005));
    assert_eq!(row.disposition, Disposition::Undecided);
    assert_eq!(row.source_category, None);
    assert_eq!(
        row.memo.as_deref(),
        Some(
            "Big shop #household\nSplit in Actual: Groceries -20.00; Uncategorized -10.05 (gift #birthday); Fuel 0.00"
        )
    );
    assert_eq!(row.tags, ["household", "birthday"]);
}

#[test]
fn starting_balances_skipped_and_balances_only_accounts_bring_no_rows() {
    let mut start = row("t0", "a", "2025-01-01", 10_000);
    start.starting_balance = true;
    let rows_in = vec![
        start,
        row("t1", "a", "2025-02-01", -100),
        row("t2", "a", "2025-01-15", -200),
        row("t3", "b", "2025-02-03", 300),
        row("t4", "b", "2025-02-04", 400),
    ];
    let mut export = export(rows_in);
    export.accounts.push(account("c", "Empty", 0));
    export.accounts[0].balance = Money::from_minor(9_700);
    export.accounts[0].bank_id = Some("fake-bank-1".to_owned());
    export.accounts[0].closed = true;

    let balances_only = AccountChoice::Create {
        name: "Savings".to_owned(),
        kind: AccountType::Savings,
        role: Role::Tracking,
        balances_only: true,
    };
    let plan = plan(
        &export,
        &choices(
            &[
                ("a", create("  Checking ")),
                ("b", balances_only),
                ("c", AccountChoice::Skip),
            ],
            &[],
        ),
    )
    .unwrap();
    assert_eq!(plan.skipped, 3);
    assert_eq!(plan.accounts.len(), 2);
    let checking = &plan.accounts[0];
    assert_eq!(
        checking.target,
        Target::New(NewAccount {
            name: "Checking".to_owned(),
            kind: AccountType::Checking,
            role: Role::Budgeted,
            balances_only: false,
            closed: true,
            bank_id: Some("fake-bank-1".to_owned()),
        })
    );
    assert_eq!(checking.balance, Money::from_minor(9_700));
    assert_eq!(checking.as_of, Some(date("2025-02-01")));
    let refs: Vec<&str> = checking
        .rows
        .iter()
        .map(|r| r.import_ref.as_str())
        .collect();
    assert_eq!(refs, ["t1", "t2"]);
    let savings = &plan.accounts[1];
    assert_eq!(savings.rows, []);
    assert_eq!(savings.as_of, Some(date("2025-02-04")));
}

#[test]
fn an_account_with_only_a_starting_balance_has_its_snapshot_today() {
    let mut start = row("t0", "a", "2025-01-01", 10_000);
    start.starting_balance = true;
    let export = export(vec![start]);
    let plan = plan(&export, &choices(&both_created(), &[])).unwrap();
    assert_eq!(plan.accounts[0].as_of, None);
    assert_eq!(plan.skipped, 1);
}

#[test]
fn a_bank_id_seen_twice_in_an_account_is_kept_once() {
    let mut first = row("t1", "a", "2025-02-01", -100);
    first.financial_id = Some("dup".to_owned());
    let mut second = first.clone();
    second.id = "t2".to_owned();
    let mut elsewhere = first.clone();
    elsewhere.id = "t3".to_owned();
    elsewhere.account = "b".to_owned();
    let mut empty = first.clone();
    empty.id = "t4".to_owned();
    empty.financial_id = Some(String::new());
    let export = export(vec![first, second, elsewhere, empty]);
    let plan = plan(
        &export,
        &choices(
            &[("a", AccountChoice::SameAs(1)), ("b", create("Savings"))],
            &[],
        ),
    )
    .unwrap();
    assert_eq!(plan.accounts[0].target, Target::Existing(1));
    let ids: Vec<Option<&str>> = rows(&plan)
        .iter()
        .map(|r| r.external_id.as_deref())
        .collect();
    assert_eq!(ids, [Some("dup"), None, None, Some("dup")]);
}

#[test]
fn choices_that_cant_go_ahead_are_refused() {
    let export = export(Vec::new());
    let refused = |accounts: &[(&str, AccountChoice)], categories: &[(&str, GoesTo)]| {
        plan(&export, &choices(accounts, categories)).unwrap_err()
    };
    assert_eq!(
        refused(&[("a", create("Checking"))], &[]),
        ChoiceError::NoChoice("Savings".to_owned())
    );
    assert_eq!(
        refused(
            &[
                ("a", create("A")),
                ("b", create("B")),
                ("x", AccountChoice::Skip)
            ],
            &[]
        ),
        ChoiceError::UnknownAccount("x".to_owned())
    );
    assert_eq!(
        refused(&[("a", create(" ")), ("b", create("B"))], &[]),
        ChoiceError::NoName("Checking".to_owned())
    );
    assert_eq!(
        refused(&[("a", create("Same")), ("b", create("same "))], &[]),
        ChoiceError::NameTaken("same".to_owned())
    );
    assert_eq!(
        refused(
            &[("a", create("OLD WALLET")), ("b", AccountChoice::Skip)],
            &[]
        ),
        ChoiceError::NameTaken("OLD WALLET".to_owned())
    );
    assert_eq!(
        refused(
            &[("a", AccountChoice::SameAs(2)), ("b", AccountChoice::Skip)],
            &[]
        ),
        ChoiceError::NoSuchAccount("Checking".to_owned())
    );
    assert_eq!(
        refused(&both_created(), &[("c2", GoesTo::Category(99))]),
        ChoiceError::NoSuchCategory("Fuel".to_owned())
    );
    assert_eq!(
        refused(&both_created(), &[("gone", GoesTo::Category(99))]),
        ChoiceError::NoSuchCategory("gone".to_owned())
    );
    assert_eq!(
        ChoiceError::NoChoice("Savings".to_owned()).to_string(),
        "Choose what happens to Savings."
    );
}

#[test]
fn dispositions_are_spelled_as_stored() {
    let mapped = Disposition::Mapped {
        category_id: 1,
        reason: String::new(),
    };
    assert_eq!(mapped.as_str(), "mapped");
    assert_eq!(Disposition::Undecided.as_str(), "undecided");
    assert_eq!(Disposition::Transfer.as_str(), "transfer");
    assert_eq!(Disposition::Uncategorized.as_str(), "uncategorized");
}

#[test]
fn normalizing_lowercases_strips_punctuation_and_collapses_spaces() {
    assert_eq!(
        normalize("  AMZN Mktp US*2K4,  Seattle  "),
        "amzn mktp us2k4 seattle"
    );
    assert_eq!(normalize("Café\tdu  Monde!"), "café du monde");
    assert_eq!(normalize("--"), "");
}

#[test]
fn tags_are_hash_words_once_each() {
    assert_eq!(
        tags(&["Birthday dinner #gift #family", "#gift, #a-b_c. x#no #"]),
        ["gift", "family", "a-b_c"]
    );
    assert_eq!(tags(&[]), Vec::<String>::new());
}

#[test]
fn the_dedupe_key_hashes_account_date_amount_and_normalized_text() {
    let key = dedupe_key(
        1,
        date("2025-03-01"),
        Money::from_minor(-1_250),
        "Corner Grocer!",
    );
    assert_eq!(key.len(), 64);
    assert!(
        key.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );
    assert_eq!(
        key,
        dedupe_key(
            1,
            date("2025-03-01"),
            Money::from_minor(-1_250),
            "corner  GROCER"
        )
    );
    for other in [
        dedupe_key(
            2,
            date("2025-03-01"),
            Money::from_minor(-1_250),
            "Corner Grocer",
        ),
        dedupe_key(
            1,
            date("2025-03-02"),
            Money::from_minor(-1_250),
            "Corner Grocer",
        ),
        dedupe_key(
            1,
            date("2025-03-01"),
            Money::from_minor(-1_251),
            "Corner Grocer",
        ),
        dedupe_key(
            1,
            date("2025-03-01"),
            Money::from_minor(-1_250),
            "Corner Grocers",
        ),
    ] {
        assert_ne!(key, other);
    }
}

#[test]
fn cents_read_as_actual_shows_them() {
    assert_eq!(cents(Money::from_minor(-5)), "-0.05");
    assert_eq!(cents(Money::from_minor(123_456)), "1234.56");
    assert_eq!(cents(Money::from_minor(i64::MIN)), "-92233720368547758.08");
}
