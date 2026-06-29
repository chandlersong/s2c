# Dockerfile
FROM chandlersong/rust_ci:1.93-slim-bookworm AS planner
WORKDIR /app
ARG LIBDIR
ENV LIBDIR=${LIBDIR}
ENV ROCKSDB_LIB_DIR=/usr/lib/${LIBDIR}
ENV OPENSSL_INCLUDE_DIR=/usr/include/openssl
ENV DUCKDB_LIB_DIR=/opt/libduckdb
ENV DUCKDB_INCLUDE_DIR=$DUCKDB_LIB_DIR
ENV LD_LIBRARY_PATH=$DUCKDB_LIB_DIR
ENV VCPKGRS_DYNAMIC=1

# Install cargo-chef to prepare dependency recipe
RUN cargo install cargo-chef --locked

# Copy manifest files for workspace and generate the recipe (fast, only depends on manifests)
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

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

# Install cargo-chef and cook the dependencies using the prepared recipe. This will cache dependency build layers.
RUN cargo install cargo-chef --locked
COPY . .
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Finally build the application binary (use APP_NAME to select the binary in the workspace)
ARG APP_NAME=test
RUN cargo build --release --bin ${APP_NAME}

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
