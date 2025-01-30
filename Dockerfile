# Dockerfile
FROM rust:1.80.1 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim AS runtime
ARG APP_NAME=test
COPY --from=builder /app/target/release/${APP_NAME} /app/app
RUN  apt update &&\
     apt install -y pkg-config libssl-dev
CMD ["/app/app"]
