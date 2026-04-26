# --- Stage 1: Base (Shared) ---
FROM rust:1.88-slim AS base
WORKDIR /app
RUN cargo install cargo-watch

# --- Stage 2: Development ---
# This stage keeps the source code linked via volumes for live-reloading
FROM base AS development
COPY . .
CMD ["cargo", "watch", "-x", "run"]

# --- Stage 3: Builder (Production prep) ---
FROM base AS builder
COPY Cargo.toml Cargo.lock .env ./
COPY src ./src
RUN cargo build --release

# --- Stage 4: Production ---
FROM debian:bookworm-slim AS production
WORKDIR /app

# --- Stage 4: Production ---
FROM debian:bookworm-slim AS production
WORKDIR /app

# 1. Install dependencies (Crucial for Rust binaries on Debian)
RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/data-collector .
COPY --from=builder /app/.env .
EXPOSE 3000
CMD ["./data-collector"]