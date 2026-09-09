FROM rust:1.98-alpine3.24 AS builder

WORKDIR /app

COPY Cargo.toml ./
COPY src ./src

RUN cargo build --release

FROM scratch

WORKDIR /app
COPY --from=builder /app/target/release/proxify /app/proxify

ENTRYPOINT ["/app/proxify"]
