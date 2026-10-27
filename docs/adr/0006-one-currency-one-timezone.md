# One currency and one timezone per household

Each install has exactly one ISO 4217 currency, chosen at setup and fixed once transactions exist, and one household IANA timezone. Amounts are stored as integer minor units of that currency (decimal places from the currency), never floats and never assuming USD. Transaction dates are plain calendar dates; the household timezone defines "today", budget window boundaries, and when scheduled jobs run. Each login picks only a display locale for formats. A household budget lives in one currency and one place, and supporting more would touch every amount, every sum and every report.

## Considered Options

- **Multi-currency with FX**: not needed by any household this targets; large cost across the ledger and reports.
- **Per-login timezones**: two people in one household would see different "today"s and different month boundaries for the same budget.

## Consequences

- Imports whose amounts can't be expressed in the household currency's minor units are refused rather than converted.
- Moving to multi-currency later would mean a currency column on accounts and amounts, and FX handling in every report.
