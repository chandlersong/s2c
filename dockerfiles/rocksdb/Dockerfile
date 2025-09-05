# Dockerfile
FROM chandlersong/rocksdb:bookworm-slim-9.9.3 AS rocksdb
FROM chandlersong/rust_ci:1.89-slim-bookworm AS builder
WORKDIR /app
COPY ../.. .
ARG LIBDIR
ENV LIBDIR=${LIBDIR}
ENV ROCKSDB_LIB_DIR=/usr/lib/${LIBDIR}
ENV OPENSSL_INCLUDE_DIR=/usr/include/openssl
ENV OPENSSL_LIB_DIR=/usr/lib/${LIBDIR}
COPY --from=rocksdb /usr/lib/${LIBDIR}/librocksdb* /usr/lib/${LIBDIR}/
RUN cargo build --release

FROM chandlersong/rust_runtime:1.89-slim-bookworm AS runtime
ARG APP_NAME=test
ENV LIBDIR=${LIBDIR}
COPY --from=builder /app/target/release/${APP_NAME} /app/app
COPY --from=rocksdb /usr/lib/${LIBDIR}/librocksdb* /usr/lib/${LIBDIR}/

ADD ../../dockerscripts/start.sh /app/start.sh
CMD ["sh","/app/start.sh"]
