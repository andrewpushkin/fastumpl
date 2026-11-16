#!/usr/bin/env bash
# Builds the Windows installer from the portable folder made by package.sh,
# running Inno Setup 7 under Wine (Docker image) or natively (Git Bash on a
# Windows machine with Inno Setup installed).
#
#   packaging/windows/installer.sh <arch-name> <inno-arch> <version> <dist-dir>
#   e.g. packaging/windows/installer.sh x86_64 x64compatible 0.1.42 dist
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

arch=$1
inno_arch=$2
version=$3
dist=$4
src="$dist/fastumpl-windows-${arch}-portable"
name="fastumpl-windows-${arch}-setup"
[ -f "$src/fastumpl.exe" ] || { echo "missing $src; build the portable folder first" >&2; exit 1; }

if command -v wine >/dev/null 2>&1; then
    # Under Wine, paths are handed over as Windows paths (Z:\...).
    win() { winepath -w "$1"; }
    iscc=(wine "${ISCC:-C:\\InnoSetup\\ISCC.exe}")
    export WINEDEBUG=-all
else
    win() { cygpath -w "$1"; }
    iscc=("${ISCC:-C:/Program Files/Inno Setup 7/ISCC.exe}")
fi

"${iscc[@]}" /Q \
    "/DAppVersion=$version" \
    "/DArch=$inno_arch" \
    "/DSourceDir=$(win "$PWD/$src")" \
    "/DOutputDir=$(win "$PWD/$dist")" \
    "/DOutputName=$name" \
    "$(win "$PWD/packaging/windows/fastumpl.iss")"
echo "$dist/$name.exe"
