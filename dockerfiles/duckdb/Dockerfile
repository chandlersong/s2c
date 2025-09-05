# Dockerfile
FROM chandlersong/rust_ci:1.89-slim-bookworm AS builder
WORKDIR /app
COPY ../.. .
ARG LIBDIR
ENV LIBDIR=${LIBDIR}
ENV ROCKSDB_LIB_DIR=/usr/lib/${LIBDIR}
ENV OPENSSL_INCLUDE_DIR=/usr/include/openssl

RUN cargo test --all-features && cargo build --release

FROM chandlersong/rust_runtime:1.89-slim-bookworm AS runtime
ARG APP_NAME=test
ENV LIBDIR=${LIBDIR}
ENV OPENSSL_LIB_DIR=/usr/lib/${LIBDIR}
ENV DUCKDB_LIB_DIR=/opt/libduckdb
ENV DUCKDB_INCLUDE_DIR=$DUCKDB_LIB_DIR
ENV LD_LIBRARY_PATH=$DUCKDB_LIB_DIR
ADD ../../dockerscripts/start.sh /app/start.sh

COPY --from=builder /opt/libduckdb /opt/libduckdb
COPY --from=builder /app/target/release/${APP_NAME} /app/app


CMD ["sh","/app/start.sh"]
