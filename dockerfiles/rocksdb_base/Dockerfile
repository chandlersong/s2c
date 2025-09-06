FROM debian:bookworm-slim
ARG LIBDIR
ENV DEBIAN_FRONTEND=noninteractive
ENV LIBDIR=${LIBDIR}
RUN apt-get update && apt-get install -y \
    build-essential \
    git \
    cmake \
    libgflags-dev \
    libsnappy-dev \
    zlib1g-dev \
    libbz2-dev \
    liblz4-dev \
    libzstd-dev \
    && rm -rf /var/lib/apt/lists/*


# 下载指定版本的 RocksDB 源码
RUN git clone --branch v9.9.3 --depth 1 https://github.com/facebook/rocksdb.git  /tmp/rocksdb

# 编译安装
WORKDIR /tmp/rocksdb
RUN git checkout 14d3046 && \
    mkdir build && cd build \
    && cmake -DCMAKE_BUILD_TYPE=Release \
        -DWITH_TESTS=OFF \
        -DWITH_TOOLS=OFF \
        -DWITH_BENCHMARK_TOOLS=OFF \
        -DROCKSDB_LITE=OFF \
        -DPORTABLE=1 \
        -DCMAKE_INSTALL_PREFIX=/usr \
        -DCMAKE_INSTALL_LIBDIR=lib/${LIBDIR} \
        .. \
    && make -j$(nproc) \
    && make install

RUN rm -rf /tmp/rocksdb
RUN apt-get clean && rm -rf /var/lib/apt/lists/*
