# syntax=docker/dockerfile:1
FROM rust:1.97.1-slim-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

RUN cargo build --release --locked

FROM gcr.io/distroless/cc-debian12:latest

COPY --from=builder /build/target/release/ctxctl /usr/local/bin/ctxctl

ENTRYPOINT ["ctxctl"]
CMD ["--help"]
