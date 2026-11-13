use domain::accounts::Role;
use domain::sync_health::Latest;

use super::*;
use crate::tests::migrated;
use crate::{
    AccountResult, ReportedAccount, RunEnd, SyncTrigger, Track, finish_sync_run,
    record_connection_accounts, save_connection, start_sync_run, track_connection_account,
};

fn at(text: &str) -> jiff::Timestamp {
    text.parse().unwrap()
}

fn reported(id: &str, link: Option<&str>) -> ReportedAccount {
    ReportedAccount {
        external_id: id.to_owned(),
        name: format!("Demo {id}"),
        institution: Some("Demo Credit Union".to_owned()),
        currency: "USD".to_owned(),
        bank_link: link.map(str::to_owned),
    }
}

async fn run(pool: &SqlitePool, connection: i64, when: &str, results: &[AccountResult]) {
    let id = start_sync_run(pool, connection, SyncTrigger::Manual, at(when))
        .await
        .unwrap();
    finish_sync_run(pool, id, &RunEnd::default(), results, at(when))
        .await
        .unwrap();
}

#[tokio::test]
async fn the_newest_result_and_the_newest_refresh_date_are_read() {
    let (_dir, pool) = migrated().await;
    let connection = save_connection(&pool, b"sealed").await.unwrap();
    record_connection_accounts(
        &pool,
        connection,
        &[reported("ACT-demo-1", Some("CON-demo-2"))],
        "USD",
        at("2026-10-20T09:00:00Z"),
    )
    .await
    .unwrap();
    let track = Track {
        role: Role::Budgeted,
        balances_only: true,
        same_as: None,
    };
    let account = track_connection_account(&pool, "ACT-demo-1", &track, "USD")
        .await
        .unwrap();

    // Tracked, never read: no result, no refresh date.
    let before = feed_account_health(&pool).await.unwrap();
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].bank_link_id.as_deref(), Some("CON-demo-2"));
    assert_eq!(before[0].bank_name.as_deref(), Some("Demo Credit Union"));
    assert!(before[0].balances_only);
    assert_eq!(before[0].facts.latest, None);
    assert_eq!(before[0].facts.balance_date, None);

    let ok = AccountResult {
        account_id: account,
        ok: true,
        error_code: None,
        error: None,
        balance_date: Some(at("2026-10-27T08:00:00Z")),
    };
    run(&pool, connection, "2026-10-27T09:00:00Z", &[ok]).await;
    let failed = AccountResult {
        account_id: account,
        ok: false,
        error_code: Some("con.auth".to_owned()),
        error: Some("Demo Credit Union needs you to sign in again.".to_owned()),
        balance_date: Some(at("2026-10-22T08:00:00Z")),
    };
    run(&pool, connection, "2026-10-28T09:00:00Z", &[failed]).await;

    let after = &feed_account_health(&pool).await.unwrap()[0];
    assert_eq!(
        after.facts.latest,
        Some(Latest {
            ok: false,
            error_code: Some("con.auth".to_owned())
        })
    );
    assert_eq!(after.facts.balance_date, Some(at("2026-10-22T08:00:00Z")));
    assert_eq!(
        after.message.as_deref(),
        Some("Demo Credit Union needs you to sign in again.")
    );

    // A failed result with no refresh date keeps the date an earlier result had.
    let absent = AccountResult {
        account_id: account,
        ok: false,
        error_code: Some("absent".to_owned()),
        error: Some("The bank didn't send this account this time.".to_owned()),
        balance_date: None,
    };
    run(&pool, connection, "2026-10-29T09:00:00Z", &[absent]).await;
    let later = &feed_account_health(&pool).await.unwrap()[0];
    assert_eq!(later.facts.balance_date, Some(at("2026-10-22T08:00:00Z")));
    assert_eq!(
        later.facts.latest.as_ref().unwrap().error_code.as_deref(),
        Some("absent")
    );
}

#[tokio::test]
async fn a_feed_account_not_tracked_is_left_out() {
    let (_dir, pool) = migrated().await;
    let connection = save_connection(&pool, b"sealed").await.unwrap();
    record_connection_accounts(
        &pool,
        connection,
        &[reported("ACT-demo-1", None)],
        "USD",
        at("2026-10-20T09:00:00Z"),
    )
    .await
    .unwrap();
    assert_eq!(feed_account_health(&pool).await.unwrap().len(), 0);
}
