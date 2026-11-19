# Development and packaging image for fastumpl.
#
# Stage `dev` (scripts/dev.sh and the Windows targets of the Makefile):
# Rust, the libraries the egui app needs to build and open a window, the
# Windows x64 / ARM64 cross toolchains, and Inno Setup 7 under Wine for the
# Windows installers.
#
# Stage `linux` (Linux targets of the Makefile): Ubuntu 22.04, so the
# binaries need only glibc 2.35 and run on most current distributions.
#
# Stage `mac`: adds osxcross for macOS builds. It needs Apple's macOS SDK,
# which may only be used on Apple hardware and is not downloaded here: put
# your own MacOSX*.sdk.tar.xz (packaged from Xcode, see packaging/macos/
# README.md) in packaging/macos/sdk/ and run `make mac`.

FROM ubuntu:26.04 AS dev

ARG DEBIAN_FRONTEND=noninteractive
# Keep in sync with rust-toolchain.toml.
ARG RUST_VERSION=1.91.0
ARG USERNAME=dev
ARG USER_UID=1000
ARG USER_GID=1000
# LLVM-based MinGW toolchain for Windows on ARM64.
ARG LLVM_MINGW=20261006
ARG LLVM_MINGW_SHA256=5f9c6ed95b2d4bdb2869a488c5fd5857fbdabcf288a0aa3eb1da43f6a08d8ab4
# Inno Setup 7 for the Windows installers (64-bit build, runs under Wine).
ARG INNO_SETUP=7.1.1
ARG INNO_SETUP_TAG=is-7_1_1
ARG INNO_SETUP_SHA256=8a5a6711c3766a6e5fb74f280ba0e3551e719ce94538d901fdbd1f8f9920b3f0

# Build tools, GUI libraries (X11, Wayland, OpenGL/EGL), GTK 3 for native
# file dialogs, fonts, a software OpenGL fallback (mesa llvmpipe), and
# packaging tools (MinGW for Windows x64, zip, ISO/DMG writer).
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        git \
        pkg-config \
        libssl-dev \
        libx11-dev \
        libxcursor-dev \
        libxrandr-dev \
        libxi-dev \
        libxkbcommon-dev \
        libxkbcommon-x11-dev \
        libwayland-dev \
        libgl-dev \
        libegl-dev \
        libgl1-mesa-dri \
        libgtk-3-dev \
        fontconfig \
        fonts-dejavu-core \
        xdg-utils \
        mingw-w64 \
        dpkg-dev \
        zip \
        xz-utils \
        genisoimage \
        wine \
        xvfb \
        xauth \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL -o /tmp/llvm-mingw.tar.xz \
        "https://github.com/mstorsjo/llvm-mingw/releases/download/${LLVM_MINGW}/llvm-mingw-${LLVM_MINGW}-ucrt-ubuntu-22.04-x86_64.tar.xz" \
    && echo "${LLVM_MINGW_SHA256}  /tmp/llvm-mingw.tar.xz" | sha256sum -c - \
    && mkdir -p /opt/llvm-mingw \
    && tar -xJf /tmp/llvm-mingw.tar.xz --strip-components=1 -C /opt/llvm-mingw \
    && rm /tmp/llvm-mingw.tar.xz

# Non-root user whose UID/GID match the host, so files written into the
# mounted repo stay owned by you. Ubuntu images ship a user "ubuntu" with
# UID 1000; remove it so the IDs are free.
RUN if id ubuntu >/dev/null 2>&1; then userdel -r ubuntu; fi \
    && if getent group ${USER_GID} >/dev/null; then groupdel $(getent group ${USER_GID} | cut -d: -f1); fi \
    && groupadd --gid ${USER_GID} ${USERNAME} \
    && useradd --uid ${USER_UID} --gid ${USER_GID} --create-home --shell /bin/bash ${USERNAME}

# llvm-mingw goes last on PATH so the GNU MinGW tools keep their names.
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH:/opt/llvm-mingw/bin \
    CC_aarch64_pc_windows_gnullvm=aarch64-w64-mingw32-clang \
    AR_aarch64_pc_windows_gnullvm=llvm-ar

# Rust via rustup, owned by the dev user so cargo can write its caches.
RUN mkdir -p ${RUSTUP_HOME} ${CARGO_HOME}/registry ${CARGO_HOME}/git \
    && chown -R ${USER_UID}:${USER_GID} ${RUSTUP_HOME} ${CARGO_HOME}

USER ${USERNAME}
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --profile minimal \
            --default-toolchain ${RUST_VERSION} \
            --component rustfmt,clippy \
            --target x86_64-pc-windows-gnu \
            --target aarch64-pc-windows-gnullvm \
    && rustc --version && cargo --version

# Inno Setup, installed into the dev user's Wine prefix at C:\InnoSetup.
ENV WINEDEBUG=-all
RUN curl -fsSL -o /tmp/innosetup.exe \
        "https://github.com/jrsoftware/issrc/releases/download/${INNO_SETUP_TAG}/innosetup-${INNO_SETUP}-x64.exe" \
    && echo "${INNO_SETUP_SHA256}  /tmp/innosetup.exe" | sha256sum -c - \
    && xvfb-run -a wine /tmp/innosetup.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- "/DIR=C:\\InnoSetup" \
    && test -f "$HOME/.wine/drive_c/InnoSetup/ISCC.exe" \
    && rm /tmp/innosetup.exe

# PlantUML samples to test the PlantUML import on, one folder per
# repository, each fetched at a pinned commit without history. See
# crates/umpl-draw/tests/puml_corpus.rs:
#   PUML_CORPUS=/opt/puml-samples cargo test -p umpl-draw --test puml_corpus -- --ignored --nocapture
ARG PUML_SAMPLES="\
spiegel-im-spiegel/plantuml-sample@fa36460ae49a865f6e44448dfdae61392eaf020a \
okunev-e/plantuml_examples@e92dc29e9cbb4843edd7f493a75d5ff411643b24 \
plantuml-stdlib/Azure-PlantUML@f360835f8794e18bed6520bb2eea56b482816d95"
USER root
RUN mkdir -p /opt/puml-samples \
    && for spec in ${PUML_SAMPLES}; do \
        repo=${spec%@*}; commit=${spec#*@}; dir=/opt/puml-samples/${repo#*/}; \
        git init --quiet "$dir" \
        && git -C "$dir" fetch --quiet --depth 1 "https://github.com/$repo" "$commit" \
        && git -C "$dir" checkout --quiet FETCH_HEAD \
        || exit 1; \
    done \
    && chown -R ${USER_UID}:${USER_GID} /opt/puml-samples

USER ${USERNAME}

WORKDIR /work
CMD ["bash"]


# ---------------------------------------------------------------------------
# The dev image with PlantUML itself (and a JDK for it), as the reference the
# PlantUML import is checked against: scripts/puml-check.sh asks it which
# corpus files have errors of their own. scripts/dev.sh runs this image; the
# packages are built in `dev`, without it. Not part of fastumpl or its
# packages.
FROM dev AS tools

ARG USERNAME=dev
ARG PLANTUML=1.2026.9
ARG PLANTUML_SHA256=aa4a1d06f8911c4b842beff4828dd955be431692b631a312d04c0e9fab9581de
USER root
RUN apt-get update && apt-get install -y --no-install-recommends default-jdk-headless \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /opt/plantuml \
    && curl -fsSL -o /opt/plantuml/plantuml.jar \
        "https://github.com/plantuml/plantuml/releases/download/v${PLANTUML}/plantuml-${PLANTUML}.jar" \
    && echo "${PLANTUML_SHA256}  /opt/plantuml/plantuml.jar" | sha256sum -c - \
    && printf '#!/bin/sh\nexec java -Djava.awt.headless=true -jar /opt/plantuml/plantuml.jar "$@"\n' \
        > /usr/local/bin/plantuml \
    && chmod +x /usr/local/bin/plantuml
USER ${USERNAME}


# ---------------------------------------------------------------------------
FROM ubuntu:22.04 AS linux

ARG DEBIAN_FRONTEND=noninteractive
ARG RUST_VERSION=1.91.0
ARG USERNAME=dev
ARG USER_UID=1000
ARG USER_GID=1000

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        git \
        pkg-config \
        libx11-dev \
        libxcursor-dev \
        libxrandr-dev \
        libxi-dev \
        libxkbcommon-dev \
        libxkbcommon-x11-dev \
        libwayland-dev \
        libgl-dev \
        libegl-dev \
        libgtk-3-dev \
        dpkg-dev \
    && rm -rf /var/lib/apt/lists/*

RUN if id ubuntu >/dev/null 2>&1; then userdel -r ubuntu; fi \
    && if getent group ${USER_GID} >/dev/null; then groupdel $(getent group ${USER_GID} | cut -d: -f1); fi \
    && groupadd --gid ${USER_GID} ${USERNAME} \
    && useradd --uid ${USER_UID} --gid ${USER_GID} --create-home --shell /bin/bash ${USERNAME}

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN mkdir -p ${RUSTUP_HOME} ${CARGO_HOME}/registry ${CARGO_HOME}/git \
    && chown -R ${USER_UID}:${USER_GID} ${RUSTUP_HOME} ${CARGO_HOME}

USER ${USERNAME}
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --profile minimal --default-toolchain ${RUST_VERSION} \
    && cargo install --locked cargo-deb@3.6.0 \
    && rm -rf ${CARGO_HOME}/registry/cache ${CARGO_HOME}/registry/src

WORKDIR /work
CMD ["bash"]


# ---------------------------------------------------------------------------
FROM dev AS mac

ARG DEBIAN_FRONTEND=noninteractive
ARG OSXCROSS_COMMIT=27d21e4977c9751d01199c7a226a6faf494c3dd9

USER root
RUN apt-get update && apt-get install -y --no-install-recommends \
        clang \
        llvm \
        lld \
        cmake \
        patch \
        python3 \
        libxml2-dev \
        zlib1g-dev \
        libbz2-dev \
        bzip2 \
        cpio \
    && rm -rf /var/lib/apt/lists/*

RUN git clone https://github.com/tpoechtrager/osxcross /opt/osxcross \
    && git -C /opt/osxcross checkout ${OSXCROSS_COMMIT}
COPY packaging/macos/sdk/ /opt/osxcross/tarballs/
RUN cd /opt/osxcross \
    && UNATTENDED=1 ./build.sh \
    && rm -rf /opt/osxcross/build /opt/osxcross/tarballs

ENV PATH=$PATH:/opt/osxcross/target/bin \
    MACOSX_DEPLOYMENT_TARGET=11.0 \
    CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER=o64-clang \
    CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=oa64-clang \
    CC_x86_64_apple_darwin=o64-clang \
    CC_aarch64_apple_darwin=oa64-clang

USER dev
RUN rustup target add x86_64-apple-darwin aarch64-apple-darwin
