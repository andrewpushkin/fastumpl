# Builds and packages fastumpl inside Docker (see Dockerfile). Packages go
# to dist/, binaries to target/.
#
#   make linux-x86_64                 build (glibc 2.35 binaries)
#   make linux-x86_64-deb             Debian/Ubuntu package
#   make linux-x86_64-packages        portable .tar.gz and .deb
#   make windows-x86_64               build
#   make windows-x86_64-packages      portable .zip and Inno Setup installer
#   make windows-arm                  build for ARM64
#   make windows-arm-packages         portable .zip and installer for ARM64
#   make mac                          universal .dmg (needs your own macOS
#                                     SDK, see packaging/macos/README.md)
#   make all                          all packages except mac
#   make test                         run the test suite
#   make clean                        remove dist/, target/ and generated files
#
# The version is 0.1.N, where N is the number of commits on main.

VERSION := $(shell scripts/version.sh)
DIST    := dist
UID     := $(shell id -u)
GID     := $(shell id -g)

IMAGE_DEV   := fastumpl-dev
IMAGE_LINUX := fastumpl-linux
IMAGE_MAC   := fastumpl-mac

# Each image keeps its own cargo cache; build output is shared in ./target,
# except Linux builds, which use their own folder (different glibc).
RUN = docker run --rm --init \
	-v "$(CURDIR)":/work -w /work \
	-v fastumpl-cargo-registry:/usr/local/cargo/registry \
	-v fastumpl-cargo-git:/usr/local/cargo/git \
	-e UMPL_VERSION=$(VERSION)

BUILD_ARGS = --build-arg USER_UID=$(UID) --build-arg USER_GID=$(GID)

.PHONY: all linux-x86_64 linux-x86_64-deb linux-x86_64-packages \
	windows-x86_64 windows-x86_64-packages windows-arm windows-arm-packages \
	mac test version clean image-dev image-linux image-mac

all: linux-x86_64-packages windows-x86_64-packages windows-arm-packages

version:
	@echo $(VERSION)

image-dev:
	docker build --target dev $(BUILD_ARGS) -t $(IMAGE_DEV) .

image-linux:
	docker build --target linux $(BUILD_ARGS) -t $(IMAGE_LINUX) .

image-mac:
	@ls packaging/macos/sdk/MacOSX*.sdk.tar.* >/dev/null 2>&1 || { \
		echo "make mac needs your own macOS SDK in packaging/macos/sdk/"; \
		echo "(Apple's license allows the SDK only on Apple hardware; see packaging/macos/README.md)"; \
		exit 1; }
	docker build --target mac $(BUILD_ARGS) -t $(IMAGE_MAC) .

LINUX = $(RUN) -e CARGO_TARGET_DIR=/work/target/linux-22.04 $(IMAGE_LINUX) packaging/linux/build.sh

linux-x86_64: image-linux
	$(LINUX) build $(VERSION) $(DIST)

linux-x86_64-deb: image-linux
	$(LINUX) deb $(VERSION) $(DIST)

linux-x86_64-packages: image-linux
	$(LINUX) tar $(VERSION) $(DIST)
	$(LINUX) deb $(VERSION) $(DIST)

WIN_X64 := x86_64-pc-windows-gnu
WIN_ARM := aarch64-pc-windows-gnullvm

windows-x86_64: image-dev
	$(RUN) $(IMAGE_DEV) cargo build --release --target $(WIN_X64) -p umpl-app -p umpl-cli

windows-x86_64-packages: windows-x86_64
	$(RUN) $(IMAGE_DEV) packaging/windows/package.sh $(WIN_X64) x86_64 $(DIST)
	$(RUN) $(IMAGE_DEV) packaging/windows/installer.sh x86_64 x64compatible $(VERSION) $(DIST)

windows-arm: image-dev
	$(RUN) $(IMAGE_DEV) cargo build --release --target $(WIN_ARM) -p umpl-app -p umpl-cli

windows-arm-packages: windows-arm
	$(RUN) $(IMAGE_DEV) packaging/windows/package.sh $(WIN_ARM) arm64 $(DIST)
	$(RUN) $(IMAGE_DEV) packaging/windows/installer.sh arm64 arm64 $(VERSION) $(DIST)

mac: image-mac
	$(RUN) $(IMAGE_MAC) packaging/macos/build.sh $(VERSION) $(DIST)

test: image-dev
	$(RUN) $(IMAGE_DEV) cargo test --workspace

# Removes everything builds generate: packages, build output for every
# target, and the website files the Pages workflow renders. Docker images
# and the cargo download cache (Docker volumes) are kept.
clean:
	rm -rf $(DIST) target site/gallery site/icon.svg site/editor.png
