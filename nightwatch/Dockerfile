# Dockerfile
FROM rust:1.80.1 as builder
WORKDIR /app
COPY . .
RUN cargo build --release --target x86_64-unknown-linux-musl

FROM alpine:3.17
COPY --from=builder /app/target/release/nightwatch /app/nightwatch
CMD ["/app/nightwatch"]
