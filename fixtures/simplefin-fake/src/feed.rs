//! What `GET /accounts` answers, per scenario. All of it is made up.

use serde_json::{Value, json};

use crate::Scenario;

/// The fake's "now": 2026-10-28 09:00:00 UTC, in epoch seconds.
pub const NOW: i64 = 1_793_178_000;
const DAY: i64 = 86_400;

/// The parameters the live Bridge honours.
pub struct Query {
    pub version2: bool,
    pub pending: bool,
    /// Epoch seconds; rows transacted before it are left out.
    pub start_date: Option<i64>,
}

impl Query {
    pub fn parse(query: &str) -> Query {
        let mut parsed = Query {
            version2: false,
            pending: false,
            start_date: None,
        };
        for pair in query.split('&') {
            match pair.split_once('=') {
                Some(("version", "2")) => parsed.version2 = true,
                Some(("pending", "1")) => parsed.pending = true,
                Some(("start-date", value)) => parsed.start_date = value.parse().ok(),
                _ => {}
            }
        }
        parsed
    }
}

struct Row {
    id: &'static str,
    /// Days before `NOW` it was transacted, and when it posted.
    transacted_days_ago: i64,
    posted_days_ago: i64,
    amount: &'static str,
    description: &'static str,
    payee: &'static str,
    pending: bool,
}

impl Row {
    fn json(&self) -> Value {
        let mut row = json!({
            "id": self.id,
            "posted": NOW - self.posted_days_ago * DAY,
            "amount": self.amount,
            "description": self.description,
            "payee": self.payee,
            "memo": "",
            "transacted_at": NOW - self.transacted_days_ago * DAY,
        });
        if self.pending {
            row["pending"] = json!(true);
        }
        row
    }
}

const fn posted(
    id: &'static str,
    days_ago: i64,
    amount: &'static str,
    description: &'static str,
    payee: &'static str,
) -> Row {
    Row {
        id,
        transacted_days_ago: days_ago,
        posted_days_ago: days_ago,
        amount,
        description,
        payee,
        pending: false,
    }
}

struct Account {
    id: &'static str,
    conn_id: &'static str,
    name: &'static str,
    balance: &'static str,
    rows: Vec<Row>,
}

/// The one extra checking row a scenario adds, if any.
fn scenario_row(scenario: Scenario, day: u32) -> Option<Row> {
    match scenario {
        Scenario::PendingPosts => Some(Row {
            id: "TRN-demo-pend-1",
            transacted_days_ago: 1,
            posted_days_ago: i64::from(day != 0),
            amount: "-23.40",
            description: "DEMO PHARMACY",
            payee: "Demo Pharmacy",
            pending: day == 0,
        }),
        Scenario::PendingAmountChanges if day == 0 => Some(Row {
            id: "TRN-demo-tab-1",
            transacted_days_ago: 1,
            posted_days_ago: 0,
            amount: "-40.00",
            description: "DEMO BISTRO",
            payee: "Demo Bistro",
            pending: true,
        }),
        Scenario::PendingAmountChanges => Some(posted(
            "TRN-demo-tab-2",
            1,
            "-48.00",
            "DEMO BISTRO",
            "Demo Bistro",
        )),
        Scenario::IdReissued => Some(posted(
            if day == 0 {
                "TRN-demo-cafe-2"
            } else {
                "TRN-demo-cafe-3"
            },
            2,
            "-4.50",
            "DEMO COFFEE ROASTERS",
            "Demo Coffee Roasters",
        )),
        _ => None,
    }
}

fn accounts(scenario: Scenario, day: u32) -> Vec<Account> {
    let mut checking = vec![
        posted(
            "TRN-demo-pay-1",
            20,
            "2400.00",
            "DEMO EMPLOYER PAYROLL",
            "Demo Employer",
        ),
        posted(
            "TRN-demo-rent-1",
            18,
            "-1200.00",
            "DEMO PROPERTIES RENT",
            "Demo Properties",
        ),
        posted(
            "TRN-demo-groc-1",
            9,
            "-86.12",
            "DEMO GROCER #12",
            "Demo Grocer",
        ),
        posted(
            "TRN-demo-power-1",
            5,
            "-64.30",
            "DEMO POWER CO",
            "Demo Power Co",
        ),
    ];
    checking.extend(scenario_row(scenario, day));
    let card = if scenario == Scenario::MissingData {
        Vec::new()
    } else {
        vec![
            posted(
                "TRN-demo-fuel-1",
                12,
                "-41.25",
                "DEMO FUEL STOP",
                "Demo Fuel Stop",
            ),
            posted("TRN-demo-cafe-1", 3, "-5.75", "DEMO CAFE", "Demo Cafe"),
        ]
    };
    let savings = vec![posted(
        "TRN-demo-int-1",
        7,
        "3.18",
        "INTEREST PAID",
        "Demo Credit Union",
    )];

    let mut all = vec![
        Account {
            id: "ACT-demo-checking",
            conn_id: "CON-demo-1",
            name: "Demo Everyday Checking",
            balance: "1157.58",
            rows: checking,
        },
        Account {
            id: "ACT-demo-card",
            conn_id: "CON-demo-1",
            name: "Demo Rewards Card",
            balance: if scenario == Scenario::WrongSign {
                "229.00"
            } else {
                "-229.00"
            },
            rows: card,
        },
        Account {
            id: "ACT-demo-savings",
            conn_id: "CON-demo-2",
            name: "Demo Savings",
            balance: "5003.18",
            rows: savings,
        },
    ];
    if scenario == Scenario::AccountMissing {
        all.retain(|account| account.id != "ACT-demo-savings");
    }
    all
}

/// The body of `GET /accounts` for `scenario` on `day`, as the query asks for it.
pub fn response(scenario: Scenario, day: u32, query: &Query) -> Value {
    let accounts = accounts(scenario, day);
    let needs_auth = scenario == Scenario::NeedsAuth;
    let errors: Vec<Value> = match scenario {
        Scenario::NeedsAuth => vec![json!({
            "code": "con.auth",
            "msg": "Demo Credit Union needs you to sign in again.",
            "conn_id": "CON-demo-2",
        })],
        Scenario::MissingData => vec![json!({
            "code": "act.missingdata",
            "msg": "Demo Rewards Card: no transactions were available.",
            "conn_id": "CON-demo-1",
            "account_id": "ACT-demo-card",
        })],
        _ => Vec::new(),
    };

    let rendered: Vec<Value> = accounts
        .iter()
        .map(|account| {
            let rows: Vec<Value> = account
                .rows
                .iter()
                .filter(|row| query.version2 && query.pending || !row.pending)
                .filter(|row| {
                    query
                        .start_date
                        .is_none_or(|start| NOW - row.transacted_days_ago * DAY >= start)
                })
                .map(Row::json)
                .collect();
            let stale = needs_auth && account.conn_id == "CON-demo-2";
            let balance_date = if stale { NOW - 6 * DAY } else { NOW - 3600 };
            if query.version2 {
                json!({
                    "id": account.id,
                    "conn_id": account.conn_id,
                    "name": account.name,
                    "currency": "USD",
                    "balance": account.balance,
                    "available-balance": account.balance,
                    "balance-date": balance_date,
                    "transactions": rows,
                    "holdings": [],
                })
            } else {
                json!({
                    "id": account.id,
                    "org": { "name": "Demo Bank", "sfin-url": "https://bridge.example.test/simplefin" },
                    "name": account.name,
                    "currency": "USD",
                    "balance": account.balance,
                    "available-balance": account.balance,
                    "balance-date": balance_date,
                    "transactions": rows,
                })
            }
        })
        .collect();

    if query.version2 {
        json!({
            "errlist": errors,
            "connections": [
                { "conn_id": "CON-demo-1", "name": "First Demo Bank", "org_id": "demo.bank.test", "org_url": "https://demo.bank.test", "sfin_url": "https://bridge.example.test/simplefin" },
                { "conn_id": "CON-demo-2", "name": "Demo Credit Union", "org_id": "demo.cu.test", "org_url": "https://demo.cu.test", "sfin_url": "https://bridge.example.test/simplefin" },
            ],
            "accounts": rendered,
        })
    } else {
        let messages: Vec<Value> = errors
            .iter()
            .filter_map(|e| e.get("msg").cloned())
            .collect();
        json!({ "errors": messages, "accounts": rendered })
    }
}
