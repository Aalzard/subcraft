#!/bin/sh
# Build SubCraft for macOS (experimental) and package an unsigned, ad-hoc signed universal app bundle:
#   dist/SubCraft-<version>-macos-universal-experimental.dmg and .zip
# Runs on a Mac (Apple Silicon or Intel) with Xcode command line tools and rustup.
# Usage: ./scripts/build-macos.sh [--skip-build]
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
[ "$(uname -s)" = Darwin ] || { echo "build-macos.sh must run on macOS" >&2; exit 1; }
version="$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' Cargo.toml | head -n 1)"
[ -n "$version" ] || { echo "could not read the version from Cargo.toml" >&2; exit 1; }
targetdir="${CARGO_TARGET_DIR:-target}"
export MACOSX_DEPLOYMENT_TARGET=11.0

if [ "$1" != "--skip-build" ]; then
    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    for t in aarch64-apple-darwin x86_64-apple-darwin; do
        echo "Building SubCraft $version for $t (dist profile)..."
        cargo build --locked --profile dist -p subcraft --target "$t"
    done
fi

name="SubCraft-$version-macos-universal-experimental"
stage="dist/$name"
app="$stage/SubCraft.app"
rm -rf "$stage" "dist/$name.dmg" "dist/$name.zip"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$stage/third-party"
lipo -create -output "$app/Contents/MacOS/subcraft" \
    "$targetdir/aarch64-apple-darwin/dist/subcraft" "$targetdir/x86_64-apple-darwin/dist/subcraft"
chmod +x "$app/Contents/MacOS/subcraft"
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist.in > "$app/Contents/Info.plist"

# Icon: SubCraft.icns from the 256 px launcher icon (sips and iconutil ship with macOS).
iconset="$(mktemp -d)/SubCraft.iconset"
mkdir -p "$iconset"
src=packaging/linux/subcraft.png
for size in 16 32 128 256 512; do
    sips -z $size $size "$src" --out "$iconset/icon_${size}x${size}.png" >/dev/null
    sips -z $((size * 2)) $((size * 2)) "$src" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/SubCraft.icns"

cp README.md LICENSE-MIT LICENSE-APACHE "$stage/"
cp packaging/macos/READ-ME-FIRST.txt "$stage/"
cp crates/ui/assets/fonts/*-LICENSE.txt "$stage/third-party/"
cp assets/starter-pack/CREDITS-CC0.txt "$stage/third-party/starter-pack-CC0.txt"
cp assets/environments/CREDITS-CC0.txt "$stage/third-party/environments-CC0.txt"

# Ad-hoc signature: Apple Silicon refuses to run completely unsigned code. This is NOT a Developer ID
# signature and the app is not notarized, so Gatekeeper still asks the user to approve it once.
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"

hdiutil create -volname "SubCraft $version" -srcfolder "$stage" -ov -format UDZO "dist/$name.dmg" >/dev/null
(cd dist && ditto -c -k --sequesterRsrc --keepParent "$name" "$name.zip")
echo "Done: dist/$name.dmg ($(du -h "dist/$name.dmg" | cut -f1)) and dist/$name.zip"
