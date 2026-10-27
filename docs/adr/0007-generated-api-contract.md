# API types are generated from Rust, and the domain crate is kept pure

The backend generates an OpenAPI document from its Rust types (`utoipa`), and the frontend's request and response types are generated from that document; CI fails if either generated file is out of date, and hand-written API types are not allowed. The Rust code is a Cargo workspace where the domain crate (money, budget math, categorizer, matching rules) cannot depend on axum, sqlx or an async runtime, so the compiler keeps business logic separate from HTTP and storage. Both serve the same goal as [ADR-0001](0001-rust-backend.md): with LLM-written code, drift between layers should be a build error, not a runtime surprise.

## Considered Options

- **Hand-written TypeScript types**: drift silently whenever a Rust struct changes.
- **One crate with module conventions**: layering enforced only by review, which is exactly what LLM-written code erodes.

## Consequences

- Every API change is a two-sided diff (Rust + regenerated TypeScript) in one pull request.
- Domain logic is unit-testable without a database or server, which is where the coverage floor applies.
