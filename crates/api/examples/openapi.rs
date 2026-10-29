//! Prints the API's OpenAPI document, for `scripts/gen-api.sh`.

use std::io::Write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let json = api::openapi().to_pretty_json()?;
    writeln!(std::io::stdout().lock(), "{json}")?;
    Ok(())
}
