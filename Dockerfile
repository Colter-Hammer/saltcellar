# syntax=docker/dockerfile:1
# The app image (docs/deploy.md): the built frontend and the `budget` binary on a distroless runtime.
# Base images stay in step with .nvmrc and rust-toolchain.toml.

# --- The frontend: web/dist ---
FROM node:24.22.1-trixie-slim AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build

# --- The Rust build, with the dependencies in their own layer (cargo-chef) ---
FROM rust:1.99.1-slim-trixie AS chef
WORKDIR /src
# rustup reads rust-toolchain.toml, so the image builds with the one pinned toolchain.
COPY rust-toolchain.toml ./
RUN cargo install cargo-chef --locked --version 0.1.78

FROM chef AS plan
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS rust
COPY --from=plan /src/recipe.json ./
RUN cargo chef cook --release --locked --bin budget --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked --bin budget && mkdir /data

# --- The runtime: no shell, no package manager, a non-root user (65532) ---
FROM gcr.io/distroless/cc-debian13:nonroot
WORKDIR /app
COPY --from=web /src/web/dist web/dist
COPY --from=rust /src/target/release/budget ./budget
# An empty /data owned by the app's user, so a new named volume starts out writable.
COPY --from=rust --chown=nonroot:nonroot /data /data
USER nonroot
ENV BUDGET_DATA_DIR=/data \
    BUDGET_LISTEN=0.0.0.0:8080
VOLUME ["/data"]
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/app/budget", "healthcheck"]
ENTRYPOINT ["/app/budget"]
