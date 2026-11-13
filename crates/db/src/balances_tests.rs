use domain::feed::SyncedRow;
use domain::import::{Disposition, Plan, PlannedAccount, Row, Target};

use super::*;
use crate::tests::{migrated, set_up};
use crate::{
    ActualCommit, SyncTrigger, account_balance, apply_synced_rows_in_run, commit_actual_import,
    start_sync_run, undo_import,
};

const CENT: Money = Money::from_minor(100);

fn at(text: &str) -> Timestamp {
    text.parse().unwrap()
}

fn date(text: &str) -> Date {
    text.parse().unwrap()
}

fn m(minor: i64) -> Money {
    Money::from_minor(minor)
}

fn row(id: &str, day: &str, minor: i64, pending: bool, text: &str) -> SyncedRow {
    SyncedRow {
        external_id: id.to_owned(),
        date: date(day),
        amount: m(minor),
        pending,
        raw_description: text.to_owned(),
        payee_text: "Demo Shop".to_owned(),
    }
}

/// A fake connection and an account on it of `kind`, with transactions unless `balances_only`.
async fn synced_account(pool: &SqlitePool, kind: &str, balances_only: bool) -> (i64, i64) {
    let connection: i64 = sqlx::query_scalar(
        "INSERT INTO connections (provider, label, secret_encrypted)
         VALUES ('simplefin', 'Demo Bridge', x'00') RETURNING id",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let account: i64 = sqlx::query_scalar(
        "INSERT INTO accounts (name, type, role, has_transactions, connection_id)
         VALUES ('Demo ' || ?1, ?1, 'budgeted', ?2, ?3) RETURNING id",
    )
    .bind(kind)
    .bind(!balances_only)
    .bind(connection)
    .fetch_one(pool)
    .await
    .unwrap();
    (connection, account)
}

/// A sync run on `connection` that applies `rows` to `account`, then records the bank's
/// `balance` refreshed at `refreshed`.
async fn sync(
    pool: &SqlitePool,
    (connection, account): (i64, i64),
    rows: &[SyncedRow],
    balance: i64,
    refreshed: &str,
) -> (i64, BalanceRecorded) {
    let now = at("2026-10-28T09:00:00Z");
    let run = start_sync_run(pool, connection, SyncTrigger::Manual, now)
        .await
        .unwrap();
    apply_synced_rows_in_run(pool, run, account, rows, date("2026-09-28"), now)
        .await
        .unwrap();
    let recorded = record_bank_balance(
        pool,
        &BankBalance {
            account_id: account,
            balance: m(balance),
            balance_date: at(refreshed),
            run_id: run,
            unit: CENT,
        },
    )
    .await
    .unwrap();
    (run, recorded)
}

/// `(as_of, balance, app posted, app with pending, run, sign corrected)`, oldest first.
type Snapshot = (String, i64, Option<i64>, Option<i64>, Option<i64>, bool);

async fn snapshots(pool: &SqlitePool, account: i64) -> Vec<Snapshot> {
    sqlx::query_as(
        "SELECT as_of, balance_minor, app_balance_posted_minor, app_balance_pending_minor,
           sync_run_id, sign_corrected
         FROM balance_snapshots WHERE account_id = ? ORDER BY as_of",
    )
    .bind(account)
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn opening_of(pool: &SqlitePool, account: i64) -> (Option<i64>, Option<String>) {
    sqlx::query_as("SELECT opening_balance_minor, opening_date FROM accounts WHERE id = ?")
        .bind(account)
        .fetch_one(pool)
        .await
        .unwrap()
}

const WRITTEN: BalanceRecorded = BalanceRecorded::Written {
    sign_corrected: false,
};

#[tokio::test]
async fn a_snapshot_is_written_when_balance_date_moves_and_replaced_the_same_utc_day() {
    let (_dir, pool) = migrated().await;
    let ids = synced_account(&pool, "checking", false).await;
    let rows = [
        row("TRN-1", "2026-10-20", -575, false, "DEMO CAFE"),
        row("TRN-2", "2026-10-27", -1200, true, "DEMO DINER"),
    ];

    // The first sync derives the opening balance: the bank's less the posted rows, dated the
    // day before the earliest.
    let (first, recorded) = sync(&pool, ids, &rows, 10_000, "2026-10-28T08:00:00Z").await;
    assert_eq!(recorded, WRITTEN);
    assert_eq!(
        opening_of(&pool, ids.1).await,
        (Some(10_575), Some("2026-10-19".to_owned()))
    );
    assert_eq!(
        snapshots(&pool, ids.1).await,
        [(
            "2026-10-28".to_owned(),
            10_000,
            Some(10_000),
            Some(8_800),
            Some(first),
            false
        )]
    );
    // The bank's balance matched the posted-only one alone: it doesn't count pending rows.
    let learned: Option<i64> =
        sqlx::query_scalar("SELECT balance_includes_pending FROM accounts WHERE id = ?")
            .bind(ids.1)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(learned, Some(0));

    // `balance-date` didn't move: nothing is written, whatever the balance says.
    let (_, recorded) = sync(&pool, ids, &rows, 1, "2026-10-28T08:00:00Z").await;
    assert_eq!(recorded, BalanceRecorded::NotMoved);
    // Nor when it goes back.
    let (_, recorded) = sync(&pool, ids, &rows, 1, "2026-10-27T23:00:00Z").await;
    assert_eq!(recorded, BalanceRecorded::NotMoved);
    assert_eq!(snapshots(&pool, ids.1).await.len(), 1);

    // A later refresh the same UTC day replaces the day's snapshot.
    let (later, recorded) = sync(&pool, ids, &rows, 9_900, "2026-10-28T23:59:59Z").await;
    assert_eq!(recorded, WRITTEN);
    assert_eq!(
        snapshots(&pool, ids.1).await,
        [(
            "2026-10-28".to_owned(),
            9_900,
            Some(10_000),
            Some(8_800),
            Some(later),
            false
        )]
    );
    // An earlier time that day than the replacing one doesn't count either.
    let (_, recorded) = sync(&pool, ids, &rows, 1, "2026-10-28T12:00:00Z").await;
    assert_eq!(recorded, BalanceRecorded::NotMoved);

    // The next UTC day is a new snapshot; the opening balance stays.
    let (next, _) = sync(&pool, ids, &rows, 10_000, "2026-10-29T00:00:00Z").await;
    let all = snapshots(&pool, ids.1).await;
    assert_eq!(all.len(), 2);
    assert_eq!(all[1].0, "2026-10-29");
    assert_eq!(all[1].4, Some(next));
    assert_eq!(
        opening_of(&pool, ids.1).await,
        (Some(10_575), Some("2026-10-19".to_owned()))
    );
}

#[tokio::test]
async fn a_first_sync_takes_the_balance_as_sent_since_rows_alone_arent_a_balance() {
    let (_dir, pool) = migrated().await;
    // A month of spending against a positive balance: opposite signs, but nothing to flip.
    let ids = synced_account(&pool, "checking", false).await;
    let rows = [row("TRN-1", "2026-10-20", -575, false, "DEMO CAFE")];
    let (_, recorded) = sync(&pool, ids, &rows, 10_000, "2026-10-28T08:00:00Z").await;
    assert_eq!(recorded, WRITTEN);
    assert_eq!(opening_of(&pool, ids.1).await.0, Some(10_575));
}

#[tokio::test]
async fn a_first_sync_without_rows_dates_the_opening_the_day_before_the_snapshot() {
    let (_dir, pool) = migrated().await;
    let ids = synced_account(&pool, "savings", false).await;
    sync(&pool, ids, &[], 500_318, "2026-10-28T08:00:00Z").await;
    assert_eq!(
        opening_of(&pool, ids.1).await,
        (Some(500_318), Some("2026-10-27".to_owned()))
    );
}

#[tokio::test]
async fn drift_is_flagged_after_two_snapshots_with_the_stepping_runs_rows_and_clears_at_zero() {
    let (_dir, pool) = migrated().await;
    let ids = synced_account(&pool, "checking", false).await;
    let cafe = row("TRN-1", "2026-10-20", -575, false, "DEMO CAFE");
    let hardware = row("TRN-3", "2026-10-27", -4_500, false, "DEMO HARDWARE");

    sync(
        &pool,
        ids,
        std::slice::from_ref(&cafe),
        10_000,
        "2026-10-26T08:00:00Z",
    )
    .await;
    assert_eq!(account_drift(&pool, ids.1).await.unwrap().flag, None);

    // The bank's balance drops by $9.50 more than the row the feed sent.
    let both = [cafe.clone(), hardware.clone()];
    sync(&pool, ids, &both, 4_550, "2026-10-27T08:00:00Z").await;
    let drift = account_drift(&pool, ids.1).await.unwrap();
    let latest = drift.latest.unwrap();
    assert_eq!(
        (latest.bank, latest.app, latest.drift),
        (m(4_550), m(5_500), m(-950))
    );
    // One snapshot off isn't a flag yet.
    assert_eq!(drift.flag, None);

    // A run where `balance-date` didn't move doesn't count.
    sync(&pool, ids, &both, 4_550, "2026-10-27T08:00:00Z").await;
    assert_eq!(account_drift(&pool, ids.1).await.unwrap().flag, None);

    // The second snapshot at the same drift flags it, since the day it stepped, with what that
    // run did to the rows.
    sync(&pool, ids, &both, 4_550, "2026-10-28T08:00:00Z").await;
    let flag = account_drift(&pool, ids.1).await.unwrap().flag.unwrap();
    assert_eq!((flag.drift, flag.since), (m(-950), date("2026-10-27")));
    let hardware_id: i64 =
        sqlx::query_scalar("SELECT id FROM transactions WHERE external_id = 'TRN-3'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        flag.suspect_rows,
        [SuspectRow {
            transaction_id: Some(hardware_id),
            change: RowChange::Added,
            date: date("2026-10-27"),
            amount: m(-4_500),
            description: "DEMO HARDWARE".to_owned(),
        }]
    );

    // It holds, keeping its date.
    sync(&pool, ids, &both, 4_550, "2026-10-29T08:00:00Z").await;
    let flag = account_drift(&pool, ids.1).await.unwrap().flag.unwrap();
    assert_eq!(flag.since, date("2026-10-27"));

    // The missing fee arrives: drift is back to zero and the flag clears itself.
    let fee = row("TRN-4", "2026-10-29", -950, false, "DEMO BANK FEE");
    let all = [cafe, hardware, fee];
    sync(&pool, ids, &all, 4_550, "2026-10-30T08:00:00Z").await;
    let drift = account_drift(&pool, ids.1).await.unwrap();
    assert_eq!(drift.flag, None);
    assert_eq!(drift.latest.unwrap().drift, m(0));
}

#[tokio::test]
async fn a_dropped_pending_row_is_a_suspect_row() {
    let (_dir, pool) = migrated().await;
    let ids = synced_account(&pool, "checking", false).await;
    let cafe = row("TRN-1", "2026-10-20", -575, false, "DEMO CAFE");
    let hold = row("TRN-P", "2026-10-25", -2_000, true, "DEMO HOTEL DEPOSIT");
    sync(
        &pool,
        ids,
        &[cafe.clone(), hold],
        10_000,
        "2026-10-26T08:00:00Z",
    )
    .await;
    // The hold goes missing twice and is dropped, while the bank's balance fell by it.
    sync(
        &pool,
        ids,
        std::slice::from_ref(&cafe),
        10_000,
        "2026-10-26T08:00:00Z",
    )
    .await;
    sync(
        &pool,
        ids,
        std::slice::from_ref(&cafe),
        8_000,
        "2026-10-27T08:00:00Z",
    )
    .await;
    sync(&pool, ids, &[cafe], 8_000, "2026-10-28T08:00:00Z").await;
    let flag = account_drift(&pool, ids.1).await.unwrap().flag.unwrap();
    assert_eq!((flag.drift, flag.since), (m(-2_000), date("2026-10-27")));
    assert_eq!(
        flag.suspect_rows,
        [SuspectRow {
            transaction_id: None,
            change: RowChange::Dropped,
            date: date("2026-10-25"),
            amount: m(-2_000),
            description: "DEMO HOTEL DEPOSIT".to_owned(),
        }]
    );
}

#[tokio::test]
async fn accepting_drift_zeroes_it_and_undo_brings_it_back_keeping_both_records() {
    let (_dir, pool) = set_up().await;
    let user: i64 = sqlx::query_scalar("SELECT id FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    let ids = synced_account(&pool, "checking", false).await;
    let cafe = row("TRN-1", "2026-10-20", -575, false, "DEMO CAFE");
    let hardware = row("TRN-3", "2026-10-27", -4_500, false, "DEMO HARDWARE");
    let both = [cafe.clone(), hardware];
    sync(&pool, ids, &[cafe], 10_000, "2026-10-26T08:00:00Z").await;
    sync(&pool, ids, &both, 4_550, "2026-10-27T08:00:00Z").await;
    sync(&pool, ids, &both, 4_550, "2026-10-28T08:00:00Z").await;
    let now = at("2026-10-28T10:00:00Z");

    // Only the flagged amount can be accepted.
    let wrong = accept_balance_adjustment(&pool, ids.1, m(-900), None, user, now).await;
    assert!(matches!(wrong, Err(AdjustmentError::NoDrift)), "{wrong:?}");
    let missing = accept_balance_adjustment(&pool, 999, m(-950), None, user, now).await;
    assert!(
        matches!(missing, Err(AdjustmentError::NotFound)),
        "{missing:?}"
    );

    let id = accept_balance_adjustment(&pool, ids.1, m(-950), Some("bank fee"), user, now)
        .await
        .unwrap();
    let drift = account_drift(&pool, ids.1).await.unwrap();
    assert_eq!(drift.flag, None);
    assert_eq!(drift.latest.as_ref().unwrap().drift, m(0));
    assert_eq!(drift.latest.unwrap().app, m(4_550));
    assert_eq!(
        drift.adjustments,
        [Adjustment {
            id,
            date: date("2026-10-28"),
            amount: m(-950),
            note: Some("bank fee".to_owned()),
            accepted_by: Some("Sam".to_owned()),
            accepted_at: now,
            undone_at: None,
        }]
    );
    // The app's balance includes it, and the next snapshot sees no drift.
    let balance = account_balance(&pool, ids.1).await.unwrap().unwrap();
    assert_eq!(balance.app_balance, Some(m(4_550)));
    sync(&pool, ids, &both, 4_550, "2026-10-29T08:00:00Z").await;
    assert_eq!(account_drift(&pool, ids.1).await.unwrap().flag, None);
    // There's nothing left to accept.
    let again = accept_balance_adjustment(&pool, ids.1, m(-950), None, user, now).await;
    assert!(matches!(again, Err(AdjustmentError::NoDrift)), "{again:?}");

    // Undo: the drift and its flag come back, since the day it stepped; the record stays.
    let later = at("2026-10-29T10:00:00Z");
    assert_eq!(
        undo_balance_adjustment(&pool, id, user, later)
            .await
            .unwrap(),
        ids.1
    );
    let drift = account_drift(&pool, ids.1).await.unwrap();
    let flag = drift.flag.unwrap();
    assert_eq!((flag.drift, flag.since), (m(-950), date("2026-10-27")));
    assert_eq!(drift.adjustments.len(), 1);
    assert_eq!(drift.adjustments[0].undone_at, Some(later));
    let balance = account_balance(&pool, ids.1).await.unwrap().unwrap();
    assert_eq!(balance.app_balance, Some(m(5_500)));
    let undone = undo_balance_adjustment(&pool, id, user, later).await;
    assert!(
        matches!(undone, Err(AdjustmentError::AlreadyUndone)),
        "{undone:?}"
    );
    let missing = undo_balance_adjustment(&pool, 999, user, later).await;
    assert!(
        matches!(missing, Err(AdjustmentError::NotFound)),
        "{missing:?}"
    );

    // Accepting again records a second adjustment beside the undone one.
    accept_balance_adjustment(&pool, ids.1, m(-950), None, user, later)
        .await
        .unwrap();
    let drift = account_drift(&pool, ids.1).await.unwrap();
    assert_eq!(drift.flag, None);
    assert_eq!(drift.adjustments.len(), 2);
    let kept: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM balance_adjustments")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(kept, 2);
}

#[tokio::test]
async fn a_wrong_sign_is_flipped_and_marked_and_the_near_zero_crossover_is_left_alone() {
    let (_dir, pool) = migrated().await;
    // A card owing $47.00, whose bank reports +47.00 from its second sync.
    let card = synced_account(&pool, "credit_card", false).await;
    let rows = [
        row("TRN-F", "2026-10-16", -4_125, false, "DEMO FUEL STOP"),
        row("TRN-C", "2026-10-25", -575, false, "DEMO CAFE"),
    ];
    let (_, recorded) = sync(&pool, card, &rows, -4_700, "2026-10-27T08:00:00Z").await;
    assert_eq!(recorded, WRITTEN);
    assert_eq!(opening_of(&pool, card.1).await.0, Some(0));
    let (_, recorded) = sync(&pool, card, &rows, 4_700, "2026-10-28T08:00:00Z").await;
    assert_eq!(
        recorded,
        BalanceRecorded::Written {
            sign_corrected: true
        }
    );
    let all = snapshots(&pool, card.1).await;
    assert_eq!((all[1].1, all[1].2, all[1].5), (-4_700, Some(-4_700), true));
    let drift = account_drift(&pool, card.1).await.unwrap();
    assert!(drift.sign_corrected);
    assert_eq!(drift.latest.unwrap().drift, m(0));

    // A card paid off, -$0.40 in the app with a refund making it +$0.50 at the bank: left alone,
    // and the drift that leaves is real.
    let paid = synced_account(&pool, "other", false).await;
    let rows = [row("TRN-R", "2026-10-25", -40, false, "DEMO REFUND")];
    sync(&pool, paid, &rows, -40, "2026-10-26T08:00:00Z").await;
    let (_, recorded) = sync(&pool, paid, &rows, 50, "2026-10-27T08:00:00Z").await;
    assert_eq!(recorded, WRITTEN);
    let drift = account_drift(&pool, paid.1).await.unwrap();
    assert!(!drift.sign_corrected);
    assert_eq!(drift.latest.unwrap().drift, m(90));
}

#[tokio::test]
async fn a_balances_only_account_gets_snapshots_only_and_a_positive_loan_is_flipped() {
    let (_dir, pool) = migrated().await;
    let loan = synced_account(&pool, "loan", true).await;
    let (_, recorded) = sync(&pool, loan, &[], 1_500_000, "2026-10-27T08:00:00Z").await;
    assert_eq!(
        recorded,
        BalanceRecorded::Written {
            sign_corrected: true
        }
    );
    assert_eq!(opening_of(&pool, loan.1).await, (None, None));
    let (_, _) = sync(&pool, loan, &[], 1_400_000, "2026-10-28T08:00:00Z").await;
    assert_eq!(
        snapshots(&pool, loan.1).await,
        [
            (
                "2026-10-27".to_owned(),
                -1_500_000,
                None,
                None,
                Some(1),
                true
            ),
            (
                "2026-10-28".to_owned(),
                -1_400_000,
                None,
                None,
                Some(2),
                true
            ),
        ]
    );
    let drift = account_drift(&pool, loan.1).await.unwrap();
    assert_eq!((drift.latest, drift.flag), (None, None));
    assert!(drift.sign_corrected);
}

fn actual_row(import_ref: &str, day: &str, amount: i64) -> Row {
    Row {
        import_ref: import_ref.to_owned(),
        date: date(day),
        amount: m(amount),
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

#[tokio::test]
async fn an_older_import_moves_the_opening_back_without_changing_drift_and_undo_restores_it() {
    let (_dir, pool) = set_up().await;
    let user: i64 = sqlx::query_scalar("SELECT id FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    let ids = synced_account(&pool, "checking", false).await;
    let cafe = row("TRN-1", "2026-10-20", -575, false, "DEMO CAFE");
    let hardware = row("TRN-3", "2026-10-27", -4_500, false, "DEMO HARDWARE");
    let both = [cafe.clone(), hardware];
    sync(&pool, ids, &[cafe], 10_000, "2026-10-26T08:00:00Z").await;
    sync(&pool, ids, &both, 4_550, "2026-10-27T08:00:00Z").await;
    let before = account_balance(&pool, ids.1).await.unwrap().unwrap();
    assert_eq!(before.app_balance, Some(m(5_500)));

    // Actual brings two months of older history; its own balance is ignored.
    let plan = Plan {
        accounts: vec![PlannedAccount {
            actual_name: "Actual checking".to_owned(),
            target: Target::Existing(ids.1),
            balance: m(123_456),
            as_of: Some(date("2026-09-15")),
            rows: vec![
                actual_row("a1", "2026-09-01", -3_000),
                actual_row("a2", "2026-09-15", 1_200),
                // A row on the opening date itself is older history too.
                actual_row("a3", "2026-10-19", -100),
            ],
        }],
        skipped: 0,
        transfers: Vec::new(),
    };
    let committed = commit_actual_import(
        &pool,
        ActualCommit {
            plan: &plan,
            file_name: "demo.zip",
            imported_by: user,
            today: date("2026-10-28"),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        opening_of(&pool, ids.1).await,
        (Some(12_475), Some("2026-08-31".to_owned()))
    );
    let after = account_balance(&pool, ids.1).await.unwrap().unwrap();
    assert_eq!(after.app_balance, Some(m(5_500)));
    // No Actual snapshot over the bank's, and drift is what it was: the next snapshot at the
    // same bank balance makes the flag.
    assert!(snapshots(&pool, ids.1).await.iter().all(|s| s.4.is_some()));
    assert_eq!(after.snapshot, Some(m(4_550)));
    sync(&pool, ids, &both, 4_550, "2026-10-28T08:00:00Z").await;
    let flag = account_drift(&pool, ids.1).await.unwrap().flag.unwrap();
    assert_eq!((flag.drift, flag.since), (m(-950), date("2026-10-27")));

    // Undoing the import puts the opening balance back.
    undo_import(&pool, committed.batch_id, true, at("2026-10-28T12:00:00Z"))
        .await
        .unwrap();
    assert_eq!(
        opening_of(&pool, ids.1).await,
        (Some(10_575), Some("2026-10-19".to_owned()))
    );
    let undone = account_balance(&pool, ids.1).await.unwrap().unwrap();
    assert_eq!(undone.app_balance, Some(m(5_500)));
}

#[tokio::test]
async fn a_back_fill_bringing_older_rows_moves_the_opening_back() {
    let (_dir, pool) = migrated().await;
    let ids = synced_account(&pool, "checking", false).await;
    let cafe = row("TRN-1", "2026-10-20", -575, false, "DEMO CAFE");
    sync(
        &pool,
        ids,
        std::slice::from_ref(&cafe),
        10_000,
        "2026-10-26T08:00:00Z",
    )
    .await;
    // A later back-fill (the account moved onto a new feed id) reaches further back.
    let older = row("TRN-0", "2026-08-10", -2_500, false, "DEMO GARAGE");
    sync(&pool, ids, &[older, cafe], 10_000, "2026-10-27T08:00:00Z").await;
    assert_eq!(
        opening_of(&pool, ids.1).await,
        (Some(13_075), Some("2026-08-09".to_owned()))
    );
    assert_eq!(
        account_drift(&pool, ids.1)
            .await
            .unwrap()
            .latest
            .unwrap()
            .drift,
        m(0)
    );
}
