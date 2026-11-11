# Fake SimpleFIN Bridge

A stand-in for SimpleFIN Bridge, for tests and local development. Every sync rule is tested against it, never against the real Bridge. Fake banks, accounts, payees and amounts only.

## Run it

```sh
cargo run -p simplefin-fake -- --scenario steady --listen 127.0.0.1:8190
```

It prints a setup token and the `BUDGET_SIMPLEFIN_CLAIM_HOSTS` value the app needs to accept it. With the app running as `just dev-api` does:

```sh
BUDGET_SECRET_KEY=$(openssl rand -base64 32) BUDGET_SIMPLEFIN_CLAIM_HOSTS=127.0.0.1:8190 just dev-api
```

Paste the token in Settings › Bank connection & sync. `POST /_fake/token` mints another one (each is good for one claim, like the real thing), `POST /_fake/next-day` moves the scenarios that change over time on a day, `POST /_fake/scenario/<name>` switches scenario, and `POST /_fake/currency/<code>` sets the currency its accounts report.

Its accounts report USD; a household in another currency refuses them, so start it with `--currency <code>` (the household's). `--public-host <host:port>` puts that address in the URLs it hands out instead of its own, for when the app reaches it by another name: the e2e tests run it as `bridge:8190` beside the app's container.

## In a test

```rust
let server = simplefin_fake::FakeServer::start(simplefin_fake::Scenario::Steady).await?;
server.setup_token();   // base64 of a claim URL, good for one claim
server.host();          // "127.0.0.1:PORT", for the claim-hosts setting
server.next_day();      // the scenarios that change over time move on
server.set_currency("EUR"); // what its accounts report (USD until set)
server.requests();      // everything the server saw (a refused token makes none)
server.follow_system_clock(); // dates count back from the real time, not NOW
```

## What it speaks

- `POST /claim/<token>`: answers the access URL (`http://user:pass@host/simplefin`). A token works once; an unknown or used one is 403.
- `GET /simplefin/accounts` (the access URL's path plus `/accounts`), Basic auth from the access URL, else 403:
  - `version=2` gives the v2 shape (`errlist`, `connections`, `accounts` with `conn_id`); without it, the v1 shape (`errors`, `org`).
  - `pending=1` includes pending rows (`"pending": true`); without it they are left out.
  - `start-date=<epoch seconds>` drops rows whose `transacted_at` is before it, cut at the exact second.
  - `end-date=<epoch seconds>` drops rows whose `transacted_at` is at or after it.
- In a test the fake's "now" is fixed at 2026-10-28 09:00:00 UTC (`simplefin_fake::NOW`); `server.follow_system_clock()` counts every date back from the real time instead, which the binary always does, so an app on the real clock finds the rows inside its windows *(agent)*. Two banks (`CON-demo-1`, `CON-demo-2`), three accounts: `ACT-demo-checking` and `ACT-demo-card` at the first, `ACT-demo-savings` at the second.

## Scenarios

| Name | The feed |
|---|---|
| `steady` | Everything fine: a month of posted rows. |
| `pending-posts` | Day 0: a pending checking row. From day 1: the same id, posted, same amount. |
| `pending-amount-changes` | Day 0: a pending row. From day 1: a posted row with a new id and a larger amount (a tip), same `transacted_at`. |
| `id-reissued` | Day 0: a posted row. From day 1: the same row under a new id. |
| `ids-reissued-together` | Day 0: two equal posted rows a day apart. From day 1: both under new ids, so neither can be told apart. |
| `pending-dropped` | Day 0: a pending checking row (a hotel deposit). From day 1: gone, never posted. |
| `posted-dropped` | Day 0: a posted checking row ten days old. From day 1: the bank stops sending it. |
| `needs-auth` | `con.auth` for `CON-demo-2` in the error list; its accounts' `balance-date` is six days old. |
| `revoked` | `GET /accounts` answers 403: the access URL was revoked. |
| `unpaid` | `GET /accounts` answers 402. |
| `missing-data` | `act.missingdata` for the card, which comes without rows. |
| `account-missing` | The savings account isn't in the response, and the error list is empty. |
| `wrong-sign` | The card's balance is reported positive although it owes money. |
| `foreign-currency` | A fourth account, `ACT-demo-travel` (Demo Travel Card) at the second bank, reports CAD (USD when the fake's currency is CAD). |

A later issue that needs another scenario adds it to `Scenario` and to this table.
