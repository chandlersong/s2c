# =============================================
# Chef Stage
# =============================================
FROM chandlersong/rocksdb:bookworm-slim-9.9.3 AS rocksdb

FROM chandlersong/rust_ci:1.96-slim-bookworm AS builder
WORKDIR /app
ENV RUSTC_WRAPPER=sccache
ENV SCCACHE_CACHE_SIZE=5G
ENV SCCACHE_DIR=/app/target/.sccache
ARG SCCACHE_GHA_ENABLED="false"
ENV SCCACHE_GHA_ENABLED=${SCCACHE_GHA_ENABLED}
ARG LIBDIR
ENV LIBDIR=${LIBDIR}
ENV ROCKSDB_LIB_DIR=/usr/lib/${LIBDIR}
ENV OPENSSL_INCLUDE_DIR=/usr/include/openssl
ENV OPENSSL_LIB_DIR=/usr/lib/${LIBDIR}

COPY --from=rocksdb /usr/lib/${LIBDIR}/librocksdb* /usr/lib/${LIBDIR}/
COPY Cargo.toml Cargo.lock ./
COPY yue/Cargo.toml yue/Cargo.toml
COPY li/Cargo.toml li/Cargo.toml
COPY yu/Cargo.toml yu/Cargo.toml

RUN mkdir -p yue/src li/src yu/src && \
    echo "pub fn __dummy_li_lib() {}" > li/src/lib.rs && \
    echo "pub fn __dummy_yue_lib() {}" > yue/src/lib.rs && \
    echo "pub fn __dummy_yu_lib() {}" > yu/src/lib.rs && \
    echo "fn main() {}" > yu/src/main.rs
RUN cargo chef prepare --recipe-path recipe.json

# =============================================
# Builder Stage
# =============================================
ARG TARGETARCH
ENV CARGO_INCREMENTAL=0
ENV CARGO_PROFILE_RELEASE_DEBUG=0

RUN cargo chef cook --release --recipe-path recipe.json --locked

ARG APP_NAME=yu_datacenter
COPY . .
RUN cargo build --release --locked --bin ${APP_NAME}

# =============================================
# Runtime Stage
# =============================================
FROM chandlersong/rust_runtime:1.96-slim-bookworm AS runtime

ARG APP_NAME=yu_datacenter
ARG LIBDIR
ENV LIBDIR=${LIBDIR}
ENV ROCKSDB_LIB_DIR=/usr/lib/${LIBDIR}
ENV OPENSSL_LIB_DIR=/usr/lib/${LIBDIR}

WORKDIR /app
COPY dockerscripts/start.sh /app/start.sh
RUN chmod +x /app/start.sh

COPY --from=rocksdb /usr/lib/${LIBDIR}/librocksdb* /usr/lib/${LIBDIR}/
COPY --from=builder /app/target/release/${APP_NAME} /app/app

CMD ["sh", "/app/start.sh"]
