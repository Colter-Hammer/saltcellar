//! Writes the fake Actual exports into a folder, `fixtures/` by default: `just fixtures`.

#[path = "../src/fake.rs"]
mod fake;

use std::io::Write;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "fixtures".to_owned()),
    );
    fake::write_all(&dir).await?;
    let mut out = std::io::stdout().lock();
    for file in fake::FILES {
        writeln!(out, "wrote {}", dir.join(file).display())?;
    }
    let fake::Expected {
        budget_name,
        accounts,
        transactions,
        categories,
        first_date,
        last_date,
    } = fake::expected();
    writeln!(
        out,
        "{budget_name}: {accounts} accounts, {transactions} transactions, {categories} categories, \
         {first_date} to {last_date}"
    )?;
    for account in fake::expected_accounts() {
        writeln!(
            out,
            "  {}: {} rows, balance {} cents{}{}",
            account.name,
            account.rows,
            account.balance,
            if account.off_budget {
                ", off budget"
            } else {
                ""
            },
            if account.closed { ", closed" } else { "" },
        )?;
    }
    Ok(())
}
