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

# Copy project manifests and prepare the recipe. We copy the repo for simplicity;
# cargo-chef will analyze manifests and produce recipe.json
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chandlersong/rust_ci:1.93-slim-bookworm AS deps
WORKDIR /app
ARG LIBDIR
ENV LIBDIR=${LIBDIR}
ENV ROCKSDB_LIB_DIR=/usr/lib/${LIBDIR}
ENV OPENSSL_INCLUDE_DIR=/usr/include/openssl
ENV DUCKDB_LIB_DIR=/opt/libduckdb
ENV DUCKDB_INCLUDE_DIR=$DUCKDB_LIB_DIR
ENV LD_LIBRARY_PATH=$DUCKDB_LIB_DIR
ENV VCPKGRS_DYNAMIC=1

# Install cargo-chef and cook dependency recipe. This produces /app/target with built deps.
RUN cargo install cargo-chef --locked
COPY . .
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# The resulting image contains compiled dependency artifacts under /app/target.
# Push this image as chandlersong/yu_deps:buildx-cache and reuse it in main builds.

