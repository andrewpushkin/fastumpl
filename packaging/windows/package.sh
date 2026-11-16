#!/usr/bin/env bash
# Makes the portable Windows zip from an existing release build (run
# `make windows-x86_64` / `make windows-arm` first; the Makefile's
# *-packages targets do both).
#
#   packaging/windows/package.sh <rust-target> <arch-name> <dist-dir>
#   e.g. packaging/windows/package.sh x86_64-pc-windows-gnu x86_64 dist
#
# Toolchain DLLs the executables need (for example libunwind.dll from
# llvm-mingw) are copied next to them, so the zip runs on a clean Windows.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

target=$1
arch=$2
dist=$3
name="fastumpl-windows-${arch}-portable"
out="$dist/$name"

bin="target/$target/release"
for exe in fastumpl.exe umpl.exe; do
    [ -f "$bin/$exe" ] || { echo "missing $bin/$exe; build first" >&2; exit 1; }
done

rm -rf "$out" "$dist/$name.zip"
mkdir -p "$out/data" "$out/examples"
cp "$bin/fastumpl.exe" "$bin/umpl.exe" "$out/"
cp packaging/windows/README.txt "$out/"
cp LICENSE "$out/LICENSE.txt"
cp assets/fonts/DejaVu-LICENSE.txt "$out/"
cp examples/*.umpl "$out/examples/"

# Runtime DLLs from the toolchain, if the executables import any.
case "$target" in
    aarch64-pc-windows-gnullvm) dll_dirs=(/opt/llvm-mingw/aarch64-w64-mingw32/bin) ;;
    x86_64-pc-windows-gnullvm) dll_dirs=(/opt/llvm-mingw/x86_64-w64-mingw32/bin) ;;
    x86_64-pc-windows-gnu) dll_dirs=(/usr/lib/gcc/x86_64-w64-mingw32/*-win32 /usr/x86_64-w64-mingw32/lib) ;;
    *) dll_dirs=() ;;
esac
for exe in "$out"/*.exe; do
    for dll in $(llvm-objdump -p "$exe" | awk '/DLL Name:/ {print $3}'); do
        for dir in "${dll_dirs[@]}"; do
            if [ -f "$dir/$dll" ] && [ ! -f "$out/$dll" ]; then
                cp "$dir/$dll" "$out/"
                echo "bundled $dll"
            fi
        done
    done
done

(cd "$dist" && zip -qr "$name.zip" "$name")
echo "$dist/$name.zip"
