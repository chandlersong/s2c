# =============================================
# Chef Stage
# =============================================
FROM chandlersong/rust_ci:1.93-slim-bookworm AS chef
WORKDIR /app

# 安装 cargo-chef
RUN cargo install cargo-chef --locked

# =============================================
# Planner Stage
# =============================================
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# =============================================
# Builder Stage
# =============================================
FROM chef AS builder

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
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --locked

# ===================== 源码编译 =====================
COPY . .
RUN cargo build --release --bin ${APP_NAME} --locked

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
COPY --from=builder /app/target/release/${APP_NAME} /app/app

CMD ["sh", "/app/start.sh"]