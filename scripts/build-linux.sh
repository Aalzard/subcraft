#!/bin/sh
# Build SubCraft for Linux and package dist/SubCraft-<version>-linux-x86_64.tar.gz
# (program, launcher, icon, install.sh, README and licenses).
# Usage: ./scripts/build-linux.sh [--skip-build]
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
version="$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' Cargo.toml | head -n 1)"
[ -n "$version" ] || { echo "could not read the version from Cargo.toml" >&2; exit 1; }

if [ "$1" != "--skip-build" ]; then
    echo "Building SubCraft $version (dist profile)..."
    cargo build --profile dist -p subcraft
fi

name="SubCraft-$version-linux-x86_64"
stage="dist/$name"
rm -rf "$stage" "dist/$name.tar.gz"
mkdir -p "$stage/third-party"
cp target/dist/subcraft "$stage/subcraft"
cp packaging/linux/subcraft.desktop packaging/linux/subcraft.png "$stage/"
cp packaging/linux/install.sh "$stage/install.sh"
chmod +x "$stage/subcraft" "$stage/install.sh"
cp README.md LICENSE-MIT LICENSE-APACHE "$stage/"
cp crates/ui/assets/fonts/*-LICENSE.txt "$stage/third-party/"
cp assets/starter-pack/CREDITS-CC0.txt "$stage/third-party/starter-pack-CC0.txt"
tar -C dist -czf "dist/$name.tar.gz" "$name"
echo "Done: dist/$name.tar.gz ($(du -h "dist/$name.tar.gz" | cut -f1))"
