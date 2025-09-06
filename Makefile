.PHONY: build_night_watch night_watch_test

APP_VERSION= 0.1.0

test_docker_file:
	docker build \
	  -f Dockerfile \
	  --build-arg APP_NAME=nightwatch \
	  --build-arg LIBDIR=aarch64-linux-gnu \
      -t chandlersong/nightwatch:0.1.0\
      .

test_build_in_docker:
	docker run \
	  --rm \
	  -it \
	  -e RUSTUP_UPDATE_ROOT=https://mirrors.ustc.edu.cn/rust-static/rustup \
	  -e RUSTUP_DIST_SERVER=https://mirrors.tuna.tsinghua.edu.cn/rustup \
	  -v $(PWD):/app \
	  -w /app \
	  chandlersong/rust_ci:1.89-slim-bookworm \
	  bash


