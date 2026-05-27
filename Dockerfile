# --- Stage 1: Base (Shared) ---
FROM rust:1.90-slim AS base
WORKDIR /app
# ADDED protobuf-compiler HERE so builder and dev stages can compile yfinance-rs
RUN apt-get update && \
    apt-get install -y pkg-config libssl-dev curl protobuf-compiler && \
    rm -rf /var/lib/apt/lists/*
    
# --- Stage 2: Development ---
FROM base AS development
# Install cargo-binstall via official script, then use it to fetch pre-compiled cargo-watch
RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
RUN cargo binstall -y cargo-watch

COPY . .
CMD ["cargo", "watch", "-x", "run"]

# --- Stage 3: Builder (Production prep) ---
FROM base AS builder
# 1. Copy ONLY the dependency manifests (wildcard for Cargo.lock in case it is absent)
COPY Cargo.toml Cargo.lock* ./
COPY migrations ./migrations

# 2. Create a dummy source file to trick Cargo into building dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs

# 3. Build dependencies (Docker caches this layer unless Cargo.toml changes)
RUN cargo build --release

# 4. Remove the dummy build artifacts so they don't interfere with your actual code
RUN rm -f target/release/deps/data_collector* target/release/data-collector*

# 5. Copy the actual source code and environment file
COPY src ./src
COPY .env ./

# 6. Update the timestamp on main.rs to force Cargo to recompile the application logic
RUN touch src/main.rs

# 7. Build the final application
RUN cargo build --release

# --- Stage 4: Production ---
FROM debian:bookworm-slim AS production
WORKDIR /app

# Install runtime SSL certificates required by native-tls (No need for protobuf here!)
RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/data-collector .
COPY --from=builder /app/.env .

EXPOSE 3000
CMD ["./data-collector"]