# ====================== Builder Stage ======================
FROM chandlersong/rust_ci:1.93-slim-bookworm AS builder

WORKDIR /app

# Build arguments
ARG LIBDIR
ARG APP_NAME=yu_datacenter
ARG TARGETARCH

# Environment setup (保持你原来的配置)
ENV LIBDIR=${LIBDIR}
ENV ROCKSDB_LIB_DIR=/usr/lib/${LIBDIR}
ENV OPENSSL_INCLUDE_DIR=/usr/include/openssl
ENV DUCKDB_LIB_DIR=/opt/libduckdb
ENV DUCKDB_INCLUDE_DIR=$DUCKDB_LIB_DIR
ENV LD_LIBRARY_PATH=$DUCKDB_LIB_DIR
ENV VCPKGRS_DYNAMIC=1
ENV OPENSSL_LIB_DIR=/usr/lib/${LIBDIR}
ENV CARGO_INCREMENTAL=0
ENV CARGO_PROFILE_RELEASE_DEBUG=0

# 安装系统依赖（如果 rust_ci 镜像已包含，可简化）
RUN apt-get update && \
    apt-get install -y clang llvm libgflags-dev protobuf-compiler libssl-dev && \
    rm -rf /var/lib/apt/lists/*

# Copy dependency files first for better caching
#COPY Cargo.toml Cargo.lock ./
COPY yue/Cargo.toml yue/Cargo.toml
COPY li/Cargo.toml li/Cargo.toml
COPY yu/Cargo.toml yu/Cargo.toml

# Create dummy source files to build dependencies
RUN mkdir -p yue/src li/src yu/src && \
    echo "fn main() {}" > yue/src/main.rs && \
    echo "fn main() {}" > li/src/lib.rs && \
    echo "fn main() {}" > yu/src/main.rs

# Pre-build dependencies (这一层会强烈缓存)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
     cargo build --release --bin ${APP_NAME} || true

# 现在复制真实源码（只有这里变化时才会重新编译）
COPY . .

# 真实构建（依赖已编译好，速度会快很多）
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release --bin ${APP_NAME}

# ====================== Runtime Stage ======================
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

# 复制启动脚本
COPY dockerscripts/start.sh /app/start.sh
RUN chmod +x /app/start.sh

# 复制 DuckDB libs 和二进制
COPY --from=builder /opt/libduckdb /opt/libduckdb
COPY --from=builder /app/target/release/${APP_NAME} /app/app

CMD ["sh", "/app/start.sh"]