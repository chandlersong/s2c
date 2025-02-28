# Dockerfile
FROM chandlersong/rocksdb:bookworm-slim-9.9.3 AS rocksdb
FROM chandlersong/rust_ci:1.85-slim-bookworm.1 AS builder
WORKDIR /app
COPY . .
ENV ROCKSDB_LIB_DIR=/usr/lib/x86_64-linux-gnu
ENV OPENSSL_INCLUDE_DIR=/usr/include/openssl
ENV OPENSSL_LIB_DIR=/usr/lib/x86_64-linux-gnu
COPY --from=rocksdb /usr/lib/x86_64-linux-gnu/librocksdb* /usr/lib/x86_64-linux-gnu/
RUN cargo build --release

FROM debian:bookworm-slim AS runtime
ARG APP_NAME=test
COPY --from=builder /app/target/release/${APP_NAME} /app/app
COPY --from=rocksdb /usr/lib/x86_64-linux-gnu/librocksdb* /usr/lib/x86_64-linux-gnu/
ENV ROCKSDB_LIB_DIR=/usr/lib/x86_64-linux-gnu
ENV OPENSSL_INCLUDE_DIR=/usr/include/openssl
ENV OPENSSL_LIB_DIR=/usr/lib/x86_64-linux-gnu
RUN  apt update &&\
     apt install -y pkg-config libssl-dev openssl ca-certificates clang llvm libgflags-dev

ADD dockerscripts/start.sh /app/start.sh
CMD ["sh","/app/start.sh"]
