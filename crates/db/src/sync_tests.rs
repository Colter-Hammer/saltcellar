use domain::accounts::Role;

use super::*;
use crate::tests::migrated;
use crate::{ReportedAccount, Track, record_connection_accounts, save_connection};

fn at(text: &str) -> Timestamp {
    text.parse().unwrap()
}

#[tokio::test]
async fn a_run_is_recorded_with_its_accounts_and_the_back_fill_is_marked() {
    let (_dir, pool) = migrated().await;
    assert_eq!(latest_sync_run(&pool).await.unwrap(), None);
    let connection = save_connection(&pool, b"sealed").await.unwrap();
    let reported = ReportedAccount {
        external_id: "ACT-demo-checking".to_owned(),
        name: "Demo Everyday Checking".to_owned(),
        institution: None,
        currency: "USD".to_owned(),
    };
    record_connection_accounts(
        &pool,
        connection,
        &[reported],
        "USD",
        at("2026-10-28T09:00:00Z"),
    )
    .await
    .unwrap();
    let track = Track {
        role: Role::Budgeted,
        balances_only: false,
        same_as: None,
    };
    let account = crate::track_connection_account(&pool, "ACT-demo-checking", &track, "USD")
        .await
        .unwrap();
    let links = tracked_feed_accounts(&pool, connection).await.unwrap();
    assert_eq!(
        links,
        [FeedLink {
            account_id: account,
            external_id: "ACT-demo-checking".to_owned(),
            balances_only: false,
            backfilled: false,
        }]
    );

    let start = at("2026-10-28T09:00:00Z");
    let run = start_sync_run(&pool, connection, SyncTrigger::Backfill, start)
        .await
        .unwrap();
    let running = latest_sync_run(&pool).await.unwrap().unwrap();
    assert_eq!(
        (running.id, running.trigger, running.finished_at),
        (run, SyncTrigger::Backfill, None)
    );
    let end = RunEnd {
        requests: 2,
        added: 6,
        updated: 0,
        error_code: None,
        error: None,
    };
    let result = AccountResult {
        account_id: account,
        ok: true,
        error_code: None,
        error: None,
        balance_date: Some(at("2026-10-28T08:00:00Z")),
    };
    let done = at("2026-10-28T09:00:02Z");
    finish_sync_run(&pool, run, &end, &[result], done)
        .await
        .unwrap();
    mark_backfilled(&pool, connection, &["ACT-demo-checking".to_owned()], done)
        .await
        .unwrap();

    let finished = latest_sync_run(&pool).await.unwrap().unwrap();
    assert_eq!(
        (
            finished.finished_at,
            finished.requests,
            finished.added,
            finished.updated
        ),
        (Some(done), 2, 6, 0)
    );
    let saved: (i64, Option<String>) = sqlx::query_as(
        "SELECT ok, provider_balance_date FROM sync_account_results WHERE sync_run_id = ?",
    )
    .bind(run)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(saved, (1, Some("2026-10-28T08:00:00.000Z".to_owned())));
    assert!(tracked_feed_accounts(&pool, connection).await.unwrap()[0].backfilled);
    assert_eq!(
        crate::connection(&pool)
            .await
            .unwrap()
            .unwrap()
            .last_sync_at,
        Some(done)
    );

    // Disconnecting takes the history with it.
    crate::disconnect(&pool).await.unwrap();
    assert_eq!(latest_sync_run(&pool).await.unwrap(), None);
}

#[test]
fn triggers_read_back() {
    for trigger in [
        SyncTrigger::Scheduled,
        SyncTrigger::Manual,
        SyncTrigger::Retry,
        SyncTrigger::CatchUp,
        SyncTrigger::Backfill,
    ] {
        assert_eq!(SyncTrigger::parse(trigger.as_str()).unwrap(), trigger);
    }
    assert!(SyncTrigger::parse("nope").is_err());
}
