# syntax=docker/dockerfile:1

FROM rust:1-bookworm AS builder
ARG ENABLE_ADMIN=0
WORKDIR /src/bank
COPY --from=passkey_rs . /src/passkey-rs
COPY Cargo.toml Cargo.lock build.rs ./
COPY proto ./proto
COPY src ./src
COPY migrations ./migrations
COPY assets ./assets
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/bank/target \
    if [ "$ENABLE_ADMIN" = "1" ]; then \
      cargo build --release --bin laefye-bank --no-default-features --features admin-api; \
    else \
      cargo build --release --bin laefye-bank --no-default-features --features customer-api; \
    fi \
    && cp target/release/laefye-bank /src/bank/laefye-bank-bin \
    && strip /src/bank/laefye-bank-bin

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates bash \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /etc/ssl/certs /etc/ssl/certs
WORKDIR /app
COPY --from=builder /src/bank/laefye-bank-bin /app/laefye-bank
COPY --from=builder /src/bank/migrations /app/migrations
COPY assets /app/assets
ENV HTTP_ADDR=0.0.0.0:8080 \
    SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt \
    BANK_ASSETS_DIR=/app/assets \
    RUST_LOG=info,passkey_rs=debug,laefye_bank=debug
EXPOSE 8080
CMD ["./laefye-bank"]
