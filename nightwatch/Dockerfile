# Dockerfile
FROM rust:1.80.1 as builder
WORKDIR /app
COPY . .
RUN cargo build --release
RUN ls /app/target/release

FROM alpine:3.17
COPY --from=builder /app/target/release/nightwatch /app/nightwatch
CMD ["/app/nightwatch"]
