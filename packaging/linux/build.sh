#!/usr/bin/env bash
# Builds and packages Linux x86-64 (run in the Ubuntu 22.04 `linux` image,
# so the binaries need only glibc 2.35).
#
#   packaging/linux/build.sh build <version> <dist-dir> compile only
#   packaging/linux/build.sh tar <version> <dist-dir>   portable .tar.gz
#   packaging/linux/build.sh deb <version> <dist-dir>   Debian package
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

mode=$1
version=$2
dist=$3
mkdir -p "$dist"

cargo build --release -p umpl-app -p umpl-cli

case "$mode" in
    build)
        echo "${CARGO_TARGET_DIR:-target}/release/fastumpl"
        ;;
    tar)
        name="fastumpl-linux-x86_64"
        out="$dist/$name"
        rm -rf "$out" "$dist/$name.tar.gz"
        mkdir -p "$out/examples"
        bin="${CARGO_TARGET_DIR:-target}/release"
        cp "$bin/fastumpl" "$bin/umpl" "$out/"
        cp packaging/linux/README.txt packaging/linux/fastumpl.desktop "$out/"
        cp assets/icon.svg "$out/fastumpl.svg"
        cp LICENSE assets/fonts/DejaVu-LICENSE.txt "$out/"
        cp examples/*.umpl "$out/examples/"
        echo "$version" > "$out/VERSION"
        tar -C "$dist" -czf "$dist/$name.tar.gz" "$name"
        echo "$dist/$name.tar.gz"
        ;;
    deb)
        cargo deb -p umpl-app --no-build --deb-version "$version" -o "$dist/fastumpl-linux-x86_64.deb"
        echo "$dist/fastumpl-linux-x86_64.deb"
        ;;
    *)
        echo "usage: $0 build|tar|deb <version> <dist-dir>" >&2
        exit 2
        ;;
esac
