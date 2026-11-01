# Build Stage
FROM rust:1.85-alpine AS builder

RUN apk add --no-cache musl-dev pkgconfig

WORKDIR /usr/src/throt

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo build --release -p throt-server

# Runtime Stage
FROM alpine:3.20

RUN apk add --no-cache ca-certificates

WORKDIR /app

COPY --from=builder /usr/src/throt/target/release/throt-server /app/throt-server

EXPOSE 8080

ENTRYPOINT ["/app/throt-server"]
