# ── Build stage ──
FROM rust:1-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
# Cache dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release 2>/dev/null; rm -rf src target/release/domofon target/release/deps/domofon-*

COPY src/ ./src/
RUN cargo build --release

# ── Runtime stage ──
FROM alpine:3

RUN apk add --no-cache ca-certificates

WORKDIR /app
COPY --from=builder /build/target/release/domofon /app/domofon
COPY public/ /app/public/

RUN mkdir -p /app/data

ENV RUST_LOG=info
ENV RUST_BACKTRACE=1

EXPOSE 3000
EXPOSE 15060/udp

CMD ["/app/domofon"]
