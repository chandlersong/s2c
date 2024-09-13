# Dockerfile
FROM rust:1.80.1 AS builder
WORKDIR /app
COPY nightwatch .
RUN cargo build --release

FROM debian:bookworm-slim AS runtime
ARG APP_NAME=test
ARG Test=test
COPY --from=builder /app/target/release/${APP_NAME} /app/nightwatch
RUN  apt update &&\
     apt install -y pkg-config libssl-dev && \
     echo ${Test}
CMD ["/app/nightwatch"]
