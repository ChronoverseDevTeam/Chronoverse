FROM alpine:3.23.3

RUN apk add --no-cache ca-certificates tzdata libgcc libstdc++ && \
    adduser -D -h /app crv

WORKDIR /app

# The binary is expected to be built by:
# cargo build -p crv-hive --release --target x86_64-unknown-linux-musl
COPY target/x86_64-unknown-linux-musl/release/crv-hive /usr/local/bin/crv-hive

RUN chmod +x /usr/local/bin/crv-hive && \
    mkdir -p /app/config /var/lib/crv-hive/repository /var/lib/crv-hive/upload-cache && \
    chown -R crv:crv /app /var/lib/crv-hive

USER crv

ENV CRV_HIVE_CONFIG_PATH=/app/config/hive.toml

EXPOSE 34560

CMD ["/usr/local/bin/crv-hive"]
