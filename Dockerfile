# syntax=docker/dockerfile:1.7
FROM rust:1.98-alpine3.24 AS builder

WORKDIR /app

COPY Cargo.toml ./
COPY src ./src

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release && \
    cp /app/target/release/proxify /app/proxify

FROM scratch

WORKDIR /app
COPY --from=builder /app/proxify /app/proxify

ENTRYPOINT ["/app/proxify"]
