# =============================================
# Chef Stage
# =============================================
FROM chandlersong/rust_ci:1.93-slim-bookworm AS chef
WORKDIR /app

# 安装 cargo-chef


# =============================================
# Planner Stage
# =============================================
FROM chef AS builder
RUN cargo install cargo-chef sccache --locked
COPY Cargo.toml Cargo.lock ./
COPY yue/Cargo.toml yue/Cargo.toml
COPY li/Cargo.toml li/Cargo.toml
COPY yu/Cargo.toml yu/Cargo.toml

ENV RUSTC_WRAPPER=sccache
ENV SCCACHE_CACHE_SIZE=5G
COPY Cargo.toml Cargo.lock ./
# Create dummy source files to build dependencies
RUN mkdir -p yue/src li/src yu/src && \
    echo "fn main() {}" > yue/src/main.rs && \
    echo "fn main() {}" > li/src/lib.rs && \
    echo "fn main() {}" > yu/src/main.rs
RUN cargo chef prepare --recipe-path recipe.json

# =============================================
# Builder Stage
# =============================================


ARG LIBDIR
ARG APP_NAME=yu_datacenter
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
RUN --mount=type=cache,target=/app/target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/root/.cache/sccache \
    cargo chef cook --release --recipe-path recipe.json --locked

# ===================== 源码编译 =====================
COPY . .
RUN --mount=type=cache,target=/app/target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/root/.cache/sccache \
    cargo build --release --bin ${APP_NAME} --locked

# =============================================
# Runtime Stage
# =============================================
FROM chandlersong/rust_runtime:1.93-slim-bookworm AS runtime

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
RUN --mount=type=cache,target=/app/target \
    cp /app/target/release/${APP_NAME} /app/app

CMD ["sh", "/app/start.sh"]