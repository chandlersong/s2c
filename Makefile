.PHONY: build_night_watch night_watch_test

APP_VERSION= 0.1.0

build_night_watch:
	docker build \
	  -f nightwatch/Dockerfile \
      -t chandlersong/nightwatch:0.1.0\
      .


night_watch_test:build_night_watch
	docker run \
	  -v ./nightwatch/conf/Settings.toml:/app/Settings.toml \
	  -e NIGHT_WATCH_CONFIG=/app/Settings.toml\
      -t chandlersong/nightwatch:0.1.0 \
      bash
