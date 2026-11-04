//! Claiming against the fake Bridge, and the fake's own behaviour (it is what every sync rule
//! will be tested against, so it gets tested too).

// The helpers below sit outside clippy's "inside a test" allowance for `unwrap`; a failing
// helper is a failing test.
#![allow(clippy::unwrap_used)]

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::Value;
use simplefin::{ClaimError, ClaimPolicy, ClaimUrl, claim};
use simplefin_fake::{FakeServer, NOW, Scenario};

/// Claims a fresh token on `server`; returns the access URL's text.
async fn claimed(server: &FakeServer) -> String {
    let policy = ClaimPolicy::only_hosts([server.host()]);
    let url = ClaimUrl::parse(&server.setup_token(), &policy).unwrap();
    tokio::task::spawn_blocking(move || claim(&url, &policy))
        .await
        .unwrap()
        .unwrap()
        .expose()
        .to_owned()
}

/// `GET <access url>/accounts?<query>`: status and JSON body.
async fn feed(access_url: &str, query: &str) -> (u16, Value) {
    let (login, rest) = access_url
        .strip_prefix("http://")
        .unwrap()
        .split_once('@')
        .unwrap();
    let url = format!("http://{rest}/accounts?{query}");
    let auth = format!("Basic {}", STANDARD.encode(login));
    tokio::task::spawn_blocking(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        let mut response = agent
            .get(&url)
            .header("Authorization", &auth)
            .call()
            .unwrap();
        let status = response.status().as_u16();
        let body = response.body_mut().read_to_string().unwrap_or_default();
        (status, serde_json::from_str(&body).unwrap_or(Value::Null))
    })
    .await
    .unwrap()
}

fn rows<'a>(body: &'a Value, account: &str) -> Vec<&'a Value> {
    body["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == account)
        .map(|a| a["transactions"].as_array().unwrap().iter().collect())
        .unwrap_or_default()
}

fn ids(rows: &[&Value]) -> Vec<String> {
    rows.iter()
        .map(|r| r["id"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn a_token_claims_once() {
    let server = FakeServer::start(Scenario::Steady).await.unwrap();
    let policy = ClaimPolicy::only_hosts([server.host()]);
    let url = ClaimUrl::parse(&server.setup_token(), &policy).unwrap();
    let again = url.clone();
    let first = {
        let policy = policy.clone();
        tokio::task::spawn_blocking(move || claim(&url, &policy))
            .await
            .unwrap()
    };
    assert!(first.unwrap().expose().starts_with("http://demo-user-"));
    let second = tokio::task::spawn_blocking(move || claim(&again, &policy))
        .await
        .unwrap();
    assert_eq!(second.unwrap_err(), ClaimError::Refused(403));
}

#[tokio::test]
async fn a_server_that_is_not_there_is_unreachable() {
    let server = FakeServer::start(Scenario::Steady).await.unwrap();
    let policy = ClaimPolicy::only_hosts([server.host()]);
    let url = ClaimUrl::parse(&server.setup_token(), &policy).unwrap();
    drop(server);
    let result = tokio::task::spawn_blocking(move || claim(&url, &policy))
        .await
        .unwrap();
    assert_eq!(result.unwrap_err(), ClaimError::Unreachable);
}

#[tokio::test]
async fn an_answer_from_another_host_is_not_an_access_url() {
    // The claim host answers with an access URL pointing elsewhere: the policy refuses it.
    let server = FakeServer::start(Scenario::Steady).await.unwrap();
    let url = ClaimUrl::parse(
        &server.setup_token(),
        &ClaimPolicy::only_hosts([server.host()]),
    )
    .unwrap();
    let narrower = ClaimPolicy::only_hosts(["127.0.0.1:1".to_owned()]);
    let result = tokio::task::spawn_blocking(move || claim(&url, &narrower))
        .await
        .unwrap();
    assert_eq!(result.unwrap_err(), ClaimError::BadAnswer);
}

#[tokio::test]
async fn the_feed_needs_a_login_the_claim_issued() {
    let server = FakeServer::start(Scenario::Steady).await.unwrap();
    let access = claimed(&server).await;
    assert_eq!(feed(&access, "version=2").await.0, 200);
    let forged = access.replace("demo-pass-1", "wrong");
    assert_eq!(feed(&forged, "version=2").await.0, 403);
    server.revoke_logins();
    assert_eq!(feed(&access, "version=2").await.0, 403);
}

#[tokio::test]
async fn version_2_has_the_v2_shape_and_pending_is_asked_for() {
    let server = FakeServer::start(Scenario::PendingPosts).await.unwrap();
    let access = claimed(&server).await;

    let (_, body) = feed(&access, "version=2&pending=1").await;
    assert!(body["errlist"].is_array() && body["connections"].is_array());
    let checking = rows(&body, "ACT-demo-checking");
    let pending = checking
        .iter()
        .find(|r| r["id"] == "TRN-demo-pend-1")
        .unwrap();
    assert_eq!(pending["pending"], true);
    let posted = checking
        .iter()
        .find(|r| r["id"] == "TRN-demo-pay-1")
        .unwrap();
    assert!(posted.get("pending").is_none(), "posted rows omit the key");
    assert_eq!(body["accounts"][0]["conn_id"], "CON-demo-1");

    let (_, without) = feed(&access, "version=2").await;
    assert!(!ids(&rows(&without, "ACT-demo-checking")).contains(&"TRN-demo-pend-1".to_owned()));

    let (_, v1) = feed(&access, "").await;
    assert!(v1["errors"].is_array() && v1.get("errlist").is_none());
}

#[tokio::test]
async fn start_date_cuts_on_the_exact_second_of_transacted_at() {
    let server = FakeServer::start(Scenario::Steady).await.unwrap();
    let access = claimed(&server).await;
    // TRN-demo-groc-1 was transacted exactly nine days before NOW.
    let at = NOW - 9 * 86_400;
    let ids_from = |start: i64| {
        let access = access.clone();
        async move {
            let (_, body) = feed(&access, &format!("version=2&pending=1&start-date={start}")).await;
            ids(&rows(&body, "ACT-demo-checking"))
        }
    };
    assert!(ids_from(at).await.contains(&"TRN-demo-groc-1".to_owned()));
    assert!(
        !ids_from(at + 1)
            .await
            .contains(&"TRN-demo-groc-1".to_owned())
    );
    assert!(
        ids_from(at + 1)
            .await
            .contains(&"TRN-demo-power-1".to_owned())
    );
}

#[tokio::test]
async fn the_requests_are_recorded() {
    let server = FakeServer::start(Scenario::Steady).await.unwrap();
    assert_eq!(server.requests(), []);
    let access = claimed(&server).await;
    feed(&access, "version=2&pending=1").await;
    let seen = server.requests();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].method, "POST");
    assert!(seen[0].path.starts_with("/claim/"));
    assert_eq!(seen[1].query, "version=2&pending=1");
}

async fn scenario_feed(scenario: Scenario, day: u32) -> (u16, Value) {
    let server = FakeServer::start(scenario).await.unwrap();
    let access = claimed(&server).await;
    for _ in 0..day {
        server.next_day();
    }
    feed(&access, "version=2&pending=1").await
}

#[tokio::test]
async fn a_pending_row_posts_under_the_same_id_or_a_new_one() {
    let (_, day0) = scenario_feed(Scenario::PendingPosts, 0).await;
    let (_, day1) = scenario_feed(Scenario::PendingPosts, 1).await;
    let find = |body: &Value| {
        rows(body, "ACT-demo-checking")
            .into_iter()
            .find(|r| r["id"] == "TRN-demo-pend-1")
            .cloned()
    };
    assert_eq!(find(&day0).unwrap()["pending"], true);
    let posted = find(&day1).unwrap();
    assert!(posted.get("pending").is_none());
    assert_eq!(posted["amount"], find(&day0).unwrap()["amount"]);

    let (_, tip0) = scenario_feed(Scenario::PendingAmountChanges, 0).await;
    let (_, tip1) = scenario_feed(Scenario::PendingAmountChanges, 1).await;
    let before = rows(&tip0, "ACT-demo-checking")
        .into_iter()
        .find(|r| r["id"] == "TRN-demo-tab-1")
        .cloned()
        .unwrap();
    let after = rows(&tip1, "ACT-demo-checking")
        .into_iter()
        .find(|r| r["id"] == "TRN-demo-tab-2")
        .cloned()
        .unwrap();
    assert_eq!(before["transacted_at"], after["transacted_at"]);
    assert_ne!(before["amount"], after["amount"]);
    assert!(!ids(&rows(&tip1, "ACT-demo-checking")).contains(&"TRN-demo-tab-1".to_owned()));
}

#[tokio::test]
async fn an_id_is_reissued() {
    let (_, day0) = scenario_feed(Scenario::IdReissued, 0).await;
    let (_, day1) = scenario_feed(Scenario::IdReissued, 1).await;
    let ids0 = ids(&rows(&day0, "ACT-demo-checking"));
    let ids1 = ids(&rows(&day1, "ACT-demo-checking"));
    assert!(ids0.contains(&"TRN-demo-cafe-2".to_owned()));
    assert!(
        ids1.contains(&"TRN-demo-cafe-3".to_owned())
            && !ids1.contains(&"TRN-demo-cafe-2".to_owned())
    );
}

#[tokio::test]
async fn the_failure_scenarios_say_what_they_say() {
    let (status, needs_auth) = scenario_feed(Scenario::NeedsAuth, 0).await;
    assert_eq!(status, 200);
    assert_eq!(needs_auth["errlist"][0]["code"], "con.auth");
    assert_eq!(needs_auth["errlist"][0]["conn_id"], "CON-demo-2");
    let savings = needs_auth["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "ACT-demo-savings")
        .unwrap();
    assert_eq!(savings["balance-date"], NOW - 6 * 86_400);

    assert_eq!(scenario_feed(Scenario::Revoked, 0).await.0, 403);
    assert_eq!(scenario_feed(Scenario::Unpaid, 0).await.0, 402);

    let (_, missing_data) = scenario_feed(Scenario::MissingData, 0).await;
    assert_eq!(missing_data["errlist"][0]["code"], "act.missingdata");
    assert_eq!(missing_data["errlist"][0]["account_id"], "ACT-demo-card");
    assert_eq!(rows(&missing_data, "ACT-demo-card").len(), 0);

    let (_, absent) = scenario_feed(Scenario::AccountMissing, 0).await;
    assert_eq!(absent["errlist"].as_array().unwrap().len(), 0);
    assert_eq!(absent["accounts"].as_array().unwrap().len(), 2);

    let (_, steady) = scenario_feed(Scenario::Steady, 0).await;
    let (_, wrong) = scenario_feed(Scenario::WrongSign, 0).await;
    let card = |body: &Value| {
        body["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == "ACT-demo-card")
            .unwrap()["balance"]
            .clone()
    };
    assert_eq!(card(&steady), "-229.00");
    assert_eq!(card(&wrong), "229.00");
}

#[test]
fn every_scenario_has_a_name_that_parses_back() {
    for scenario in Scenario::ALL {
        assert_eq!(Scenario::parse(scenario.name()), Some(scenario));
    }
    assert_eq!(Scenario::parse("nope"), None);
}
