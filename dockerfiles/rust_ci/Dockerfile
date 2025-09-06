FROM chandlersong/rocksdb:bookworm-slim-9.9.3 AS rocksdb
FROM rust:1.89-slim-bookworm AS rust
ARG LIBDIR
ARG TARGETARCH
ENV LIBDIR=${LIBDIR}
ENV TARGETARCH=${TARGETARCH}
RUN apt-get  update &&\
    apt-get  install -y clang llvm libgflags-dev protobuf-compiler libssl-dev unzip wget
# install rocksdb


COPY --from=rocksdb /usr/lib/${LIBDIR}/librocksdb* /usr/lib/${LIBDIR}/
ENV ROCKSDB_LIB_DIR=/usr/lib/${LIBDIR}

RUN apt update && \
    apt install -y unzip wget

# install duckdb
RUN wget https://github.com/duckdb/duckdb/releases/download/v1.3.2/libduckdb-linux-${TARGETARCH}.zip &&\
    unzip libduckdb-linux-${TARGETARCH}.zip  -d /opt/libduckdb &&\
    rm libduckdb-linux-${TARGETARCH}.zip
    
ENV DUCKDB_LIB_DIR=/opt/libduckdb
ENV DUCKDB_INCLUDE_DIR=$DUCKDB_LIB_DIR
ENV LD_LIBRARY_PATH=$DUCKDB_LIB_DIR
ENV VCPKGRS_DYNAMIC=1

ENV OPENSSL_INCLUDE_DIR=/usr/include/openssl
ENV OPENSSL_LIB_DIR=/usr/lib/${LIBDIR}

