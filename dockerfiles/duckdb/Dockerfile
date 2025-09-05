# Dockerfile
FROM chandlersong/rust_ci:1.89-slim-bookworm AS builder
WORKDIR /app
COPY ../.. .
ARG LIBDIR
ENV LIBDIR=${LIBDIR}
ENV ROCKSDB_LIB_DIR=/usr/lib/${LIBDIR}
ENV OPENSSL_INCLUDE_DIR=/usr/include/openssl
ENV OPENSSL_LIB_DIR=/usr/lib/${LIBDIR}
COPY --from=rocksdb /usr/lib/${LIBDIR}/librocksdb* /usr/lib/${LIBDIR}/
RUN cargo test --all-features && cargo build --release

FROM chandlersong/rust_runtime:1.89-slim-bookworm AS runtime
ARG APP_NAME=test
ENV LIBDIR=${LIBDIR}

RUN wget https://github.com/duckdb/duckdb/releases/download/v1.3.2/libduckdb-osx-universal.zip **\
    unzip libduckdb-osx-universal.zip -d /opt/libduckdb
ENV DUCKDB_LIB_DIR=/opt/libduckdb
ENV DUCKDB_INCLUDE_DIR=$DUCKDB_LIB_DIR
ENV LD_LIBRARY_PATH=$DUCKDB_LIB_DIR

COPY --from=builder /app/target/release/${APP_NAME} /app/app

ADD ../../dockerscripts/start.sh /app/start.sh
CMD ["sh","/app/start.sh"]
