//! `simplefin-fake`: runs the fake Bridge for local development.
//! `cargo run -p simplefin-fake -- --scenario steady --listen 127.0.0.1:8190`
//! `--currency EUR` makes its accounts report the household's currency; `--public-host bridge:8190`
//! puts that address in the URLs it hands out, for when the app reaches it by another name.

use std::io::Write;
use std::net::SocketAddr;
use std::process::ExitCode;

use simplefin_fake::{FakeServer, Scenario};

fn usage() -> String {
    let names: Vec<&str> = Scenario::ALL.iter().map(|s| s.name()).collect();
    format!(
        "usage: simplefin-fake [--scenario <name>] [--listen <addr>] [--currency <code>] [--public-host <host:port>]\nscenarios: {}",
        names.join(", ")
    )
}

/// What the command line asks for.
struct Args {
    scenario: Scenario,
    listen: SocketAddr,
    currency: Option<String>,
    public_host: Option<String>,
}

/// The scenario, address, currency and public host from the command line.
fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut scenario = Scenario::Steady;
    let mut listen = SocketAddr::from(([127, 0, 0, 1], 8190));
    let (mut currency, mut public_host) = (None, None);
    let mut args = args;
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(usage)?;
        match flag.as_str() {
            "--scenario" => {
                scenario = Scenario::parse(&value)
                    .ok_or_else(|| format!("no scenario {value:?}\n{}", usage()))?;
            }
            "--listen" => {
                listen = value
                    .parse()
                    .map_err(|_| format!("{value:?} isn't an address\n{}", usage()))?;
            }
            "--currency" => currency = Some(value),
            "--public-host" => public_host = Some(value),
            _ => return Err(usage()),
        }
    }
    Ok(Args {
        scenario,
        listen,
        currency,
        public_host,
    })
}

#[tokio::main]
async fn main() -> ExitCode {
    let Args {
        scenario,
        listen,
        currency,
        public_host,
    } = match parse_args(std::env::args().skip(1)) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("simplefin-fake: {message}");
            return ExitCode::from(2);
        }
    };

    let server = match FakeServer::start_as(listen, public_host, scenario).await {
        Ok(server) => server,
        Err(error) => {
            eprintln!("simplefin-fake: can't listen on {listen}: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(currency) = currency {
        server.set_currency(&currency);
    }
    let mut out = std::io::stdout();
    let _ = writeln!(out, "Fake SimpleFIN Bridge on http://{}", server.host());
    let _ = writeln!(out, "Scenario: {}", scenario.name());
    let _ = writeln!(
        out,
        "Run the app with BUDGET_SIMPLEFIN_CLAIM_HOSTS={}",
        server.host()
    );
    let _ = writeln!(out, "Setup token: {}", server.setup_token());
    let _ = writeln!(
        out,
        "More: POST /_fake/token (a new token), /_fake/next-day, /_fake/scenario/<name>, /_fake/currency/<code>"
    );
    let _ = out.flush();
    let _ = tokio::signal::ctrl_c().await;
    ExitCode::SUCCESS
}
