//! `simplefin-fake`: runs the fake Bridge for local development.
//! `cargo run -p simplefin-fake -- --scenario steady --listen 127.0.0.1:8190`

use std::io::Write;
use std::net::SocketAddr;
use std::process::ExitCode;

use simplefin_fake::{FakeServer, Scenario};

fn usage() -> String {
    let names: Vec<&str> = Scenario::ALL.iter().map(|s| s.name()).collect();
    format!(
        "usage: simplefin-fake [--scenario <name>] [--listen <addr>]\nscenarios: {}",
        names.join(", ")
    )
}

/// The scenario and address from the command line.
fn parse_args(args: impl Iterator<Item = String>) -> Result<(Scenario, SocketAddr), String> {
    let mut scenario = Scenario::Steady;
    let mut listen = SocketAddr::from(([127, 0, 0, 1], 8190));
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
            _ => return Err(usage()),
        }
    }
    Ok((scenario, listen))
}

#[tokio::main]
async fn main() -> ExitCode {
    let (scenario, listen) = match parse_args(std::env::args().skip(1)) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("simplefin-fake: {message}");
            return ExitCode::from(2);
        }
    };

    let server = match FakeServer::start_on(listen, scenario).await {
        Ok(server) => server,
        Err(error) => {
            eprintln!("simplefin-fake: can't listen on {listen}: {error}");
            return ExitCode::FAILURE;
        }
    };
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
        "More: POST /_fake/token (a new token), /_fake/next-day, /_fake/scenario/<name>"
    );
    let _ = out.flush();
    let _ = tokio::signal::ctrl_c().await;
    ExitCode::SUCCESS
}
