.PHONY: build_night_watch_release

APP_VERSION= 0.1.0
PLATFORMS="linux/amd64,linux/arm64,linux/arm/v7"

build_night_watch_release:
	docker build \
	  -f nightwatch/Dockerfile \
      -t chandlersong/nightwatch:$(APP_VERSION)\
      .

