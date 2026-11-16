#!/usr/bin/env bash
# Builds the macOS universal (Intel + Apple Silicon) app and .dmg.
#
#   packaging/macos/build.sh <version> <dist-dir>
#
# Runs on a Mac (uses lipo, codesign and hdiutil) or in the Docker `mac`
# image (osxcross, llvm-lipo and genisoimage).
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

version=$1
dist=$2
targets=(x86_64-apple-darwin aarch64-apple-darwin)
export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-11.0}

for t in "${targets[@]}"; do
    cargo build --release --target "$t" -p umpl-app -p umpl-cli
done

lipo_tool() {
    if command -v lipo >/dev/null 2>&1; then
        lipo "$@"
    elif command -v llvm-lipo >/dev/null 2>&1; then
        llvm-lipo "$@"
    else
        "$(compgen -c | grep -m1 -- '-apple-darwin.*-lipo$')" "$@"
    fi
}

work="$dist/macos"
app="$work/dmg/fastumpl.app"
rm -rf "$work"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$work/dmg/examples"
for bin in fastumpl umpl; do
    lipo_tool -create -output "$app/Contents/MacOS/$bin" \
        "target/x86_64-apple-darwin/release/$bin" \
        "target/aarch64-apple-darwin/release/$bin"
done
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
cp packaging/macos/fastumpl.icns "$app/Contents/Resources/"
printf 'APPL????' > "$app/Contents/PkgInfo"

# Apple Silicon only runs signed code; an ad-hoc signature is enough for a
# downloaded app opened with right-click > Open.
if command -v codesign >/dev/null 2>&1; then
    codesign --force --deep --sign - "$app"
fi

cp examples/*.umpl "$work/dmg/examples/"
cp LICENSE "$work/dmg/LICENSE.txt"
ln -s /Applications "$work/dmg/Applications"

dmg="$dist/fastumpl-macos-universal.dmg"
rm -f "$dmg"
if command -v hdiutil >/dev/null 2>&1; then
    hdiutil create -volname fastumpl -srcfolder "$work/dmg" -ov -format UDZO "$dmg"
else
    # Hybrid ISO 9660 / HFS image; macOS mounts it like any .dmg.
    genisoimage -quiet -V fastumpl -D -R -apple -hfs -no-pad -o "$dmg" "$work/dmg"
fi
echo "$dmg"
