# Carboy —— 常用目标
PREFIX ?= /usr/local
DESTDIR ?=

.PHONY: all build run test install rpm deb clean

all: build

build:
	cargo build --release

run:
	cargo run --release

test:
	cargo test

install: build
	install -Dm755 target/release/carboy $(DESTDIR)$(PREFIX)/bin/carboy
	install -Dm644 data/io.github.carboy.MkDesktop.desktop \
		$(DESTDIR)$(PREFIX)/share/applications/io.github.carboy.MkDesktop.desktop
	install -Dm644 data/io.github.carboy.MkDesktop.svg \
		$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/io.github.carboy.MkDesktop.svg
	install -Dm644 data/io.github.carboy.MkDesktop.metainfo.xml \
		$(DESTDIR)$(PREFIX)/share/metainfo/io.github.carboy.MkDesktop.metainfo.xml

rpm:
	./packaging/build-rpm.sh

deb:
	./packaging/build-deb.sh

clean:
	cargo clean
