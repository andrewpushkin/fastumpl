# Release packages, built inside Docker (see Dockerfile). Results go to dist/.
#
#   make linux-x86_64              portable .tar.gz
#   make linux-x86_64-deb          Debian/Ubuntu package
#   make windows-x86_64            portable .zip
#   make windows-x86_64-installer  Inno Setup installer
#   make windows-arm               portable .zip for ARM64
#   make windows-arm-installer     Inno Setup installer for ARM64
#   make mac                       universal .dmg (needs your own macOS SDK,
#                                  see packaging/macos/README.md)
#   make all                       everything except mac
#   make test                      run the test suite
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

.PHONY: all linux-x86_64 linux-x86_64-deb windows-x86_64 windows-x86_64-installer \
	windows-arm windows-arm-installer mac test version clean \
	image-dev image-linux image-mac

all: linux-x86_64 linux-x86_64-deb windows-x86_64 windows-x86_64-installer \
	windows-arm windows-arm-installer

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

linux-x86_64: image-linux
	$(RUN) -e CARGO_TARGET_DIR=/work/target/linux-22.04 $(IMAGE_LINUX) \
		packaging/linux/build.sh tar $(VERSION) $(DIST)

linux-x86_64-deb: image-linux
	$(RUN) -e CARGO_TARGET_DIR=/work/target/linux-22.04 $(IMAGE_LINUX) \
		packaging/linux/build.sh deb $(VERSION) $(DIST)

windows-x86_64: image-dev
	$(RUN) $(IMAGE_DEV) packaging/windows/build.sh x86_64-pc-windows-gnu x86_64 $(DIST)

windows-x86_64-installer: windows-x86_64
	$(RUN) $(IMAGE_DEV) packaging/windows/installer.sh x86_64 x64compatible $(VERSION) $(DIST)

windows-arm: image-dev
	$(RUN) $(IMAGE_DEV) packaging/windows/build.sh aarch64-pc-windows-gnullvm arm64 $(DIST)

windows-arm-installer: windows-arm
	$(RUN) $(IMAGE_DEV) packaging/windows/installer.sh arm64 arm64 $(VERSION) $(DIST)

mac: image-mac
	$(RUN) $(IMAGE_MAC) packaging/macos/build.sh $(VERSION) $(DIST)

test: image-dev
	$(RUN) $(IMAGE_DEV) cargo test --workspace

clean:
	rm -rf $(DIST)
