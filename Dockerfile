# Stage 1: Build
FROM rust:1.85.1-slim as builder

WORKDIR /app

COPY Cargo.toml Cargo.lock .env ./
COPY src ./src

RUN cargo build --release

# Stage 2: Run
FROM alpine:latest as runner

WORKDIR /app

RUN apt-get update && apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/data-collector .
COPY .env ./

EXPOSE 3000

CMD ["./data-collector"]