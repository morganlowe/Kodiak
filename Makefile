.PHONY: all build pkg deb install dev check test clean

VERSION := $(shell sed -n 's/.*"version":[[:space:]]*"\([^"]*\)".*/\1/p' src-tauri/tauri.conf.json | head -n1)
ARCH_BUNDLE_DIR := src-tauri/target/release/bundle/arch

all: build

## Standard full build output: Tauri bundles (.deb, .rpm) + native Arch/CachyOS package (.pkg.tar.zst)
build:
	npx --no-install tauri build
	@$(MAKE) pkg-bundle

## Build Arch/CachyOS package (.pkg.tar.zst) and copy into src-tauri/target/release/bundle/arch/
pkg: pkg-bundle

pkg-bundle:
	BUILDDIR=/tmp/kodiak-makepkg makepkg -sf --noconfirm
	mkdir -p $(ARCH_BUNDLE_DIR)
	cp -f kodiak-*.pkg.tar.* $(ARCH_BUNDLE_DIR)/

## Build Debian/Ubuntu packages (.deb) for Kodiak and kodiak-projectswiki
deb:
	./scripts/build_deb.sh

## One-command compile & install on any Arch/CachyOS machine
install:
	BUILDDIR=/tmp/kodiak-makepkg makepkg -sfci --noconfirm

dev:
	npx --no-install tauri dev

check:
	cargo check --manifest-path src-tauri/Cargo.toml

test:
	cargo test --manifest-path src-tauri/Cargo.toml

clean:
	rm -rf pkg/ kodiak-*.pkg.tar.* foreman-*.pkg.tar.* dist/
	cargo clean --manifest-path src-tauri/Cargo.toml
