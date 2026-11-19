#!/usr/bin/env bash
# Run a command inside the fastumpl development container.
#
#   scripts/dev.sh                  interactive shell
#   scripts/dev.sh cargo test       run one command
#   scripts/dev.sh cargo run -p umpl-app -- file.umpl   opens a window
#
# The image is (re)built on each call; Docker's cache makes that instant
# when the Dockerfile has not changed.
set -euo pipefail

IMAGE=fastumpl-dev
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

docker build --quiet --target tools \
    --build-arg USER_UID="$(id -u)" \
    --build-arg USER_GID="$(id -g)" \
    -t "$IMAGE" "$ROOT" >/dev/null

args=(
    --rm
    --init
    -v "$ROOT":/work
    -w /work
    # Downloaded crates survive between runs; build output lives in ./target.
    -v fastumpl-cargo-registry:/usr/local/cargo/registry
    -v fastumpl-cargo-git:/usr/local/cargo/git
)
[ -t 0 ] && [ -t 1 ] && args+=(-it)

# Display forwarding.
if [ -d /mnt/wslg ]; then
    # Windows WSL2 with WSLg. Use X11 only: WSLg's Wayland socket fails
    # from inside a container ("Broken pipe"), while X11 works.
    args+=(
        -v /tmp/.X11-unix:/tmp/.X11-unix
        -v /mnt/wslg:/mnt/wslg
        -e DISPLAY="${DISPLAY:-:0}"
        -e XDG_RUNTIME_DIR=/mnt/wslg/runtime-dir
    )
    # GPU acceleration through WSL's D3D12 driver, when available.
    if [ -e /dev/dxg ]; then
        args+=(--device /dev/dxg -v /usr/lib/wsl:/usr/lib/wsl:ro
               -e LD_LIBRARY_PATH=/usr/lib/wsl/lib)
    fi
else
    # Plain Linux desktop.
    if [ -n "${DISPLAY:-}" ] && [ -d /tmp/.X11-unix ]; then
        args+=(-v /tmp/.X11-unix:/tmp/.X11-unix -e DISPLAY="$DISPLAY")
    fi
    if [ -n "${WAYLAND_DISPLAY:-}" ] && [ -n "${XDG_RUNTIME_DIR:-}" ]; then
        args+=(-v "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY:/tmp/runtime/$WAYLAND_DISPLAY"
               -e WAYLAND_DISPLAY="$WAYLAND_DISPLAY"
               -e XDG_RUNTIME_DIR=/tmp/runtime)
    fi
    [ -d /dev/dri ] && args+=(--device /dev/dri)
fi

if [ $# -eq 0 ]; then
    set -- bash
fi
exec docker run "${args[@]}" "$IMAGE" "$@"
