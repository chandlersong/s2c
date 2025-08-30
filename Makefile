.PHONY: build_night_watch night_watch_test

APP_VERSION= 0.1.0

test_docker_file:
	docker build \
	  -f Dockerfile \
	  --build-arg APP_NAME=nightwatch \
      -t chandlersong/nightwatch:0.1.0\
      .
