# Dockerfile
FROM rust:1.80.1 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim as runtime
COPY --from=builder /app/target/release/nightwatch /app/nightwatch
RUN  apt update &&\
     apt install -y pkg-config libssl-dev
CMD ["/app/nightwatch"]
