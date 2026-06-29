# Dockerfile

# Optional deps cache image produced by build-deps-cache workflow
FROM chandlersong/yu_deps:buildx-cache AS deps

FROM chandlersong/rust_ci:1.93-slim-bookworm AS builder
WORKDIR /app
ARG LIBDIR
ENV LIBDIR=${LIBDIR}
ENV ROCKSDB_LIB_DIR=/usr/lib/${LIBDIR}
ENV OPENSSL_INCLUDE_DIR=/usr/include/openssl
ENV DUCKDB_LIB_DIR=/opt/libduckdb
ENV DUCKDB_INCLUDE_DIR=$DUCKDB_LIB_DIR
ENV LD_LIBRARY_PATH=$DUCKDB_LIB_DIR
ENV VCPKGRS_DYNAMIC=1

# Copy repository root (context expected to be repo root)
COPY ../.. .

# Try to reuse compiled dependency artifacts from deps image if available
COPY --from=deps /app/target /app/target

# Build final binary; if deps exist in /app/target this will be much faster
RUN cargo build --release

FROM chandlersong/rust_runtime:1.93-slim-bookworm AS runtime
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
