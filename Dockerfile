# Dockerfile
FROM chandlersong/rocksdb:bookworm-slim-9.9.3 AS rocksdb
FROM rust:1.80.1 AS builder
WORKDIR /app
COPY . .
ENV ROCKSDB_LIB_DIR=/usr/lib/x86_64-linux-gnu
COPY --from=Rocksdb /usr/lib/x86_64-linux-gnu/librocksdb* /usr/lib/x86_64-linux-gnu/
RUN RUN  apt update &&\
         apt install -y pkg-config libssl-dev openssl ca-certificates clang llvm libgflags-dev protobuf-compiler\
    cargo build --release

FROM debian:bookworm-slim AS runtime
ARG APP_NAME=test
COPY --from=builder /app/target/release/${APP_NAME} /app/app
COPY --from=Rocksdb /usr/lib/x86_64-linux-gnu/librocksdb* /usr/lib/x86_64-linux-gnu/
ENV ROCKSDB_LIB_DIR=/usr/lib/x86_64-linux-gnu
RUN  apt update &&\
     apt install -y pkg-config libssl-dev openssl ca-certificates clang llvm libgflags-dev

ADD dockerscripts/start.sh /app/start.sh
CMD ["sh","/app/start.sh"]
