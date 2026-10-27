# Rust backend (axum + sqlx)

The backend is Rust with axum and sqlx, even though the owner's strongest stack is Python/FastAPI. The code is almost entirely written and maintained by an LLM, with the owner doing QA, so the deciding factor is a strict compiler that catches generated-code mistakes (types, nulls, error paths, and SQL checked against the schema at build time via sqlx macros) before they reach runtime. Performance is not the reason; at household scale every candidate is fast enough.

## Considered Options

- **Python/FastAPI**: owner's best-known stack, but most errors surface only at runtime, i.e. during the owner's QA.
- **TypeScript full-stack**: shares API types with the Vue frontend, but weaker runtime guarantees than Rust.

## Consequences

- Categorization logic stays in-process in Rust; methods must be ones that are practical to hand-write without Python's ML ecosystem.
- Slower compiles and code the owner reads less fluently; keep it plain and well commented.
