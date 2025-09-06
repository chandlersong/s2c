FROM debian:bookworm-slim AS runtime
ARG LIBDIR
ENV LIBDIR=${LIBDIR}
ENV ROCKSDB_LIB_DIR=/usr/lib/${LIBDIR}
ENV OPENSSL_INCLUDE_DIR=/usr/include/openssl
ENV OPENSSL_LIB_DIR=/usr/lib/${LIBDIR}
RUN  apt update &&\
     apt install -y pkg-config libssl-dev openssl ca-certificates clang llvm libgflags-dev
