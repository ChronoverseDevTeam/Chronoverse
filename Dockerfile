FROM rust:alpine AS builder

RUN apk add --no-cache musl-dev pkgconfig openssl-dev

WORKDIR /app

COPY Cargo.toml ./
COPY crates/ crates/

RUN cargo build --release -p crv-core

FROM alpine:3.20

RUN apk add --no-cache ca-certificates curl

COPY --from=builder /app/target/release/crv-core /usr/local/bin/crv-core

EXPOSE 3000
HEALTHCHECK --interval=10s --timeout=3s --retries=5 \
    CMD curl -f http://localhost:3000/api/v1/health || exit 1

CMD ["crv-core"]
