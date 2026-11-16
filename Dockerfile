# Development image for fastumpl: Rust toolchain plus the system libraries
# the egui/eframe desktop app needs to build and open a window.
#
# Use scripts/dev.sh rather than calling docker directly; it mounts the repo,
# keeps a cargo cache volume and forwards the display (WSLg, X11 or Wayland).

FROM ubuntu:26.04

ARG DEBIAN_FRONTEND=noninteractive
# Keep in sync with rust-toolchain.toml.
ARG RUST_VERSION=1.91.0
ARG USERNAME=dev
ARG USER_UID=1000
ARG USER_GID=1000

# Build tools, GUI libraries (X11, Wayland, OpenGL/EGL), GTK 3 for native
# file dialogs, fonts, and a software OpenGL fallback (mesa llvmpipe).
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
    && rm -rf /var/lib/apt/lists/*

# Non-root user whose UID/GID match the host, so files written into the
# mounted repo stay owned by you. Ubuntu images ship a user "ubuntu" with
# UID 1000; remove it so the IDs are free.
RUN if id ubuntu >/dev/null 2>&1; then userdel -r ubuntu; fi \
    && if getent group ${USER_GID} >/dev/null; then groupdel $(getent group ${USER_GID} | cut -d: -f1); fi \
    && groupadd --gid ${USER_GID} ${USERNAME} \
    && useradd --uid ${USER_UID} --gid ${USER_GID} --create-home --shell /bin/bash ${USERNAME}

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

# Rust via rustup, owned by the dev user so cargo can write its caches.
RUN mkdir -p ${RUSTUP_HOME} ${CARGO_HOME}/registry ${CARGO_HOME}/git \
    && chown -R ${USER_UID}:${USER_GID} ${RUSTUP_HOME} ${CARGO_HOME}

USER ${USERNAME}
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --profile minimal \
            --default-toolchain ${RUST_VERSION} \
            --component rustfmt,clippy \
            --target x86_64-pc-windows-gnu \
    && rustc --version && cargo --version

# Packaging: cargo-deb builds the Linux .deb. The Windows GNU target above
# (with mingw-w64) checks that Windows-only code compiles; release builds
# for Windows use MSVC on GitHub Actions.
RUN cargo install --locked cargo-deb@3.6.0 \
    && rm -rf ${CARGO_HOME}/registry/cache ${CARGO_HOME}/registry/src

WORKDIR /work
CMD ["bash"]
