# Stage 1: Build
FROM rust:1.100.0-alpine3.24 AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app

COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,uid=0,gid=0 \
    --mount=type=cache,target=/app/target,uid=0,gid=0 \
    cargo build --release --locked --bin ur-scanner && \
    cp target/release/ur-scanner /usr/local/bin/ur-scanner

# Stage 2: Minimal Runtime
FROM alpine:3.24
RUN addgroup -S appgroup && adduser -S appuser -G appgroup
COPY --from=builder /usr/local/bin/ur-scanner /usr/local/bin/ur-scanner

USER appuser
ENTRYPOINT ["/usr/local/bin/ur-scanner"]
