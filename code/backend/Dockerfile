# syntax=docker/dockerfile:1

# ---- Stage 1: build -----------------------------------------------------------
# Pinned major version; works for both amd64 (Proxmox) and arm64 (Raspberry Pi).
FROM rust:1-bookworm AS builder

WORKDIR /app

# Cache dependency compilation: build with stub sources first, so `cargo build`
# only recompiles our crate (not all deps) when source — but not Cargo.toml —
# changes. SQLite is statically bundled by sqlx (libsqlite3-sys), so no system
# SQLite dev package is required here.
COPY Cargo.toml ./
COPY Cargo.lock* ./
RUN mkdir src \
    && echo "fn main() {}" > src/main.rs \
    && echo "" > src/lib.rs \
    && cargo build --release \
    && rm -rf src

# Now copy the real sources and build the actual binary.
COPY migrations ./migrations
COPY src ./src
# Bust the dependency-cache layer's stale artifacts for our own crate.
RUN touch src/main.rs src/lib.rs \
    && cargo build --release --bin trading-journal-api

# ---- Stage 2: runtime ---------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# Only CA certificates are needed at runtime (no OpenSSL dependency).
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Run as an unprivileged user.
RUN useradd --system --create-home --uid 10001 appuser
WORKDIR /app

# Persistent SQLite data lives here.
RUN mkdir -p /app/data && chown -R appuser:appuser /app
VOLUME ["/app/data"]

COPY --from=builder /app/target/release/trading-journal-api /usr/local/bin/trading-journal-api

USER appuser

ENV DATABASE_URL=sqlite:///app/data/journal.db \
    BIND_ADDR=0.0.0.0:8080 \
    RUST_LOG=info,tower_http=info,sqlx=warn

EXPOSE 8080

# JWT_SECRET and ADMIN_PASSWORD_HASH must be provided at runtime
# (docker run --env-file .env ...). The container will refuse to start without them.
ENTRYPOINT ["trading-journal-api"]
