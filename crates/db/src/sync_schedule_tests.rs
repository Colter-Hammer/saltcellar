use domain::accounts::Role;

use super::*;
use crate::tests::migrated;
use crate::{
    AccountResult, ReportedAccount, RunEnd, Track, finish_sync_run, record_connection_accounts,
    save_connection, start_sync_run,
};

fn at(text: &str) -> Timestamp {
    text.parse().unwrap()
}

#[tokio::test]
async fn sync_settings_read_back_and_turning_raw_off_deletes_the_kept_ones() {
    let (_dir, pool) = migrated().await;
    assert_eq!(sync_settings(&pool).await.unwrap(), SyncSettings::default());
    set_sync_settings(&pool, "07:41", true).await.unwrap();
    assert_eq!(
        sync_settings(&pool).await.unwrap(),
        SyncSettings {
            time: Some("07:41".to_owned()),
            keep_raw_responses: true
        }
    );
    let value: String = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'sync.time'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(value, "\"07:41\"");

    let connection = save_connection(&pool, b"sealed").await.unwrap();
    let run = start_sync_run(
        &pool,
        connection,
        SyncTrigger::Manual,
        at("2026-10-28T09:00:00Z"),
    )
    .await
    .unwrap();
    keep_raw_response(&pool, run, 0, at("2026-10-28T09:00:01Z"), b"sealed body")
        .await
        .unwrap();
    set_sync_settings(&pool, "07:41", false).await.unwrap();
    let kept: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_raw_responses")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(kept, 0);
}

/// A connection with one tracked account: `(connection, account)`.
async fn connected_account(pool: &SqlitePool) -> (i64, i64) {
    let connection = save_connection(pool, b"sealed").await.unwrap();
    let reported = ReportedAccount {
        external_id: "ACT-demo-checking".to_owned(),
        name: "Demo Everyday Checking".to_owned(),
        institution: None,
        currency: "USD".to_owned(),
        bank_link: None,
    };
    record_connection_accounts(
        pool,
        connection,
        &[reported],
        "USD",
        at("2026-07-01T09:00:00Z"),
    )
    .await
    .unwrap();
    let track = Track {
        role: Role::Budgeted,
        balances_only: false,
        same_as: None,
    };
    let account = crate::track_connection_account(pool, "ACT-demo-checking", &track, "USD")
        .await
        .unwrap();
    (connection, account)
}

/// A finished run of `trigger` with one request, started at `started`, failed with `error` if
/// given, with a kept raw response; resolves to its id.
async fn run_at(
    pool: &SqlitePool,
    connection: i64,
    account: i64,
    trigger: SyncTrigger,
    started: &str,
    error: Option<&str>,
) -> i64 {
    let started = at(started);
    let error = error.map(str::to_owned);
    let run = start_sync_run(pool, connection, trigger, started)
        .await
        .unwrap();
    let end = RunEnd {
        requests: 1,
        error_code: error.clone(),
        error: error.clone(),
        ..RunEnd::default()
    };
    let result = AccountResult {
        account_id: account,
        ok: error.is_none(),
        error_code: None,
        error: None,
        balance_date: None,
    };
    let finished = started + SignedDuration::from_secs(2);
    finish_sync_run(pool, run, &end, &[result], finished)
        .await
        .unwrap();
    keep_raw_response(pool, run, 0, started, b"sealed")
        .await
        .unwrap();
    run
}

#[tokio::test]
async fn the_history_lists_runs_newest_first_with_their_accounts_and_prunes() {
    let (_dir, pool) = migrated().await;
    let (connection, account) = connected_account(&pool).await;

    let old = run_at(
        &pool,
        connection,
        account,
        SyncTrigger::Scheduled,
        "2026-07-01T11:13:00Z",
        None,
    )
    .await;
    let scheduled = run_at(
        &pool,
        connection,
        account,
        SyncTrigger::Scheduled,
        "2026-10-20T11:13:00Z",
        None,
    )
    .await;
    let manual = run_at(
        &pool,
        connection,
        account,
        SyncTrigger::Manual,
        "2026-10-28T09:00:00Z",
        Some("unreachable"),
    )
    .await;
    run_at(
        &pool,
        connection,
        account,
        SyncTrigger::Backfill,
        "2026-10-28T09:10:00Z",
        None,
    )
    .await;

    assert_eq!(
        last_scheduled_sync(&pool).await.unwrap(),
        Some(at("2026-10-20T11:13:00Z"))
    );
    assert_eq!(
        last_successful_sync(&pool).await.unwrap(),
        Some(at("2026-10-28T09:10:02Z"))
    );
    assert_eq!(
        last_finished_sync(&pool).await.unwrap(),
        Some(at("2026-10-28T09:00:02Z"))
    );
    assert_eq!(
        requests_since(&pool, at("2026-10-27T09:10:00Z"))
            .await
            .unwrap(),
        [
            (at("2026-10-28T09:00:00Z"), 1),
            (at("2026-10-28T09:10:00Z"), 1)
        ]
    );

    let history = sync_history(&pool).await.unwrap();
    let ids: Vec<i64> = history.iter().map(|r| r.run.id).collect();
    assert_eq!(ids[1..], [manual, scheduled, old]);
    assert_eq!(history[1].run.error_code.as_deref(), Some("unreachable"));
    assert_eq!(
        history[1].accounts,
        [RunAccount {
            account_id: account,
            name: "Demo Everyday Checking".to_owned(),
            ok: false,
            error_code: None,
            error: None,
            balance_date: None,
        }]
    );

    prune_sync_history(
        &pool,
        at("2026-10-28T10:00:00Z"),
        SignedDuration::from_hours(90 * 24),
        SignedDuration::from_hours(7 * 24),
    )
    .await
    .unwrap();
    let ids: Vec<i64> = sync_history(&pool)
        .await
        .unwrap()
        .iter()
        .map(|r| r.run.id)
        .collect();
    assert_eq!(ids[1..], [manual, scheduled]);
    let kept: Vec<i64> =
        sqlx::query_scalar("SELECT sync_run_id FROM sync_raw_responses ORDER BY 1")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(kept, [manual, ids[0]]);
}
