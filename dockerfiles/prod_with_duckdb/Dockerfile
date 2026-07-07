# =============================================
# Chef Stage
# =============================================
FROM chandlersong/rust_ci:1.96-slim-bookworm AS builder
WORKDIR /app
ENV RUSTC_WRAPPER=sccache
ENV SCCACHE_CACHE_SIZE=5G
ENV SCCACHE_DIR=/app/target/.sccache
ARG SCCACHE_GHA_ENABLED="false"
ENV SCCACHE_GHA_ENABLED=${SCCACHE_GHA_ENABLED}
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

# =============================================
# Builder Stage
# =============================================


ARG LIBDIR

ARG TARGETARCH

# 环境变量（保持你原来的配置）
ENV LIBDIR=${LIBDIR}
ENV ROCKSDB_LIB_DIR=/usr/lib/${LIBDIR}
ENV OPENSSL_INCLUDE_DIR=/usr/include/openssl
ENV OPENSSL_LIB_DIR=/usr/lib/${LIBDIR}
ENV DUCKDB_LIB_DIR=/opt/libduckdb
ENV DUCKDB_INCLUDE_DIR=$DUCKDB_LIB_DIR
ENV LD_LIBRARY_PATH=$DUCKDB_LIB_DIR
ENV VCPKGRS_DYNAMIC=1

ENV CARGO_INCREMENTAL=0
ENV CARGO_PROFILE_RELEASE_DEBUG=0

# ===================== 依赖预编译 =====================
RUN cargo chef cook --release --recipe-path recipe.json --locked

# ===================== 源码编译 =====================
ARG APP_NAME=yu_datacenter
COPY . .
RUN cargo build --release --locked

# =============================================
# Runtime Stage
# =============================================
FROM chandlersong/rust_runtime:1.96-slim-bookworm AS runtime

ARG APP_NAME=yu_datacenter
ARG LIBDIR

ENV LIBDIR=${LIBDIR}
ENV ROCKSDB_LIB_DIR=/usr/lib/${LIBDIR}
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