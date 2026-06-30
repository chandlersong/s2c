# =============================================
# RocksDB source stage
# =============================================
FROM chandlersong/rocksdb:bookworm-slim-9.9.3 AS rocksdb-src

# =============================================
# Builder Stage
# =============================================
FROM rust:1.96-slim-bookworm AS builder
ARG LIBDIR
ARG TARGETARCH
ARG APP_NAME=yu_datacenter
ARG SCCACHE_GHA_ENABLED="false"

ENV LIBDIR=${LIBDIR}
ENV TARGETARCH=${TARGETARCH}

RUN apt-get update && \
    apt-get install -y clang llvm libgflags-dev protobuf-compiler libssl-dev unzip wget build-essential pkg-config && \
    rm -rf /var/lib/apt/lists/*

# Install RocksDB from pre-built image
COPY --from=rocksdb-src /usr/lib/${LIBDIR}/librocksdb* /usr/lib/${LIBDIR}/
ENV ROCKSDB_LIB_DIR=/usr/lib/${LIBDIR}

# Install DuckDB
RUN wget https://github.com/duckdb/duckdb/releases/download/v1.5.3/libduckdb-linux-${TARGETARCH}.zip && \
    unzip libduckdb-linux-${TARGETARCH}.zip -d /opt/libduckdb && \
    rm libduckdb-linux-${TARGETARCH}.zip

ENV DUCKDB_LIB_DIR=/opt/libduckdb
ENV DUCKDB_INCLUDE_DIR=$DUCKDB_LIB_DIR
ENV LD_LIBRARY_PATH=$DUCKDB_LIB_DIR
ENV VCPKGRS_DYNAMIC=1
ENV OPENSSL_INCLUDE_DIR=/usr/include/openssl
ENV OPENSSL_LIB_DIR=/usr/lib/${LIBDIR}

# Install cargo tools (cached as a separate layer)
RUN cargo install cargo-chef sccache

WORKDIR /app
ENV RUSTC_WRAPPER=sccache
ENV SCCACHE_CACHE_SIZE=5G
ENV SCCACHE_DIR=/app/target/.sccache
ENV SCCACHE_GHA_ENABLED=${SCCACHE_GHA_ENABLED}
ENV CARGO_INCREMENTAL=0
ENV CARGO_PROFILE_RELEASE_DEBUG=0

COPY Cargo.toml Cargo.lock ./
COPY yue/Cargo.toml yue/Cargo.toml
COPY li/Cargo.toml li/Cargo.toml
COPY yu/Cargo.toml yu/Cargo.toml

# Create dummy source files to build dependencies
RUN mkdir -p yue/src li/src yu/src && \
    # yue is a workspace library dependency and must provide a lib target (lib.rs). \
    # Otherwise cargo will warn: "ignoring invalid dependency `yue` which is missing a lib target". \
    echo "pub fn __dummy_yue_lib() {}" > yue/src/lib.rs && \
    echo "pub fn __dummy_yue_lib() {}" > li/src/lib.rs && \
    echo "fn main() {}" > li/src/lib.rs && \
    echo "fn main() {}" > yue/src/lib.rs && \
    echo "fn main() {}" > yu/src/main.rs
RUN cargo chef prepare --recipe-path recipe.json

# ===================== 依赖预编译 =====================
RUN cargo chef cook --release --recipe-path recipe.json --locked

# ===================== 源码编译 =====================
COPY . .
RUN cargo build --release --locked

# =============================================
# Runtime Stage
# =============================================
FROM debian:bookworm-slim AS runtime

ARG APP_NAME=yu_datacenter
ARG LIBDIR

RUN apt-get update && \
    apt-get install -y pkg-config libssl-dev openssl ca-certificates clang llvm libgflags-dev && \
    rm -rf /var/lib/apt/lists/*

ENV LIBDIR=${LIBDIR}
ENV ROCKSDB_LIB_DIR=/usr/lib/${LIBDIR}
ENV OPENSSL_INCLUDE_DIR=/usr/include/openssl
ENV OPENSSL_LIB_DIR=/usr/lib/${LIBDIR}
ENV DUCKDB_LIB_DIR=/opt/libduckdb
ENV DUCKDB_INCLUDE_DIR=$DUCKDB_LIB_DIR
ENV LD_LIBRARY_PATH=$DUCKDB_LIB_DIR

WORKDIR /app

COPY dockerscripts/start.sh /app/start.sh
RUN chmod +x /app/start.sh

COPY --from=builder /opt/libduckdb /opt/libduckdb
COPY --from=builder /app/target/release/${APP_NAME} /app/app

CMD ["sh", "/app/start.sh"]