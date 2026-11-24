#!/bin/sh
# Package SubCraft as dist/SubCraft-<version>-linux-<arch>.flatpak, a single-file bundle that
# `flatpak install --user <file>` installs (it pulls the freedesktop runtime from Flathub).
# Needs target/dist/subcraft (cargo build --profile dist -p subcraft, or ./scripts/build-linux.sh),
# flatpak, flatpak-builder 1.4 or newer (older ones call appstream-compose, which current SDKs
# lack), and the flathub remote:
#   flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
# Usage: ./scripts/build-flatpak.sh
# FLATPAK_BUILDER="flatpak run --command=flatpak-builder org.flatpak.Builder" uses Flathub's builder
# (install it with: flatpak install --user flathub org.flatpak.Builder).
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
app_id=io.github.aalzard.SubCraft
version="$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' Cargo.toml | head -n 1)"
[ -n "$version" ] || { echo "could not read the version from Cargo.toml" >&2; exit 1; }
builder="${FLATPAK_BUILDER:-flatpak-builder}"
command -v "${builder%% *}" >/dev/null || { echo "${builder%% *} is missing: install flatpak and flatpak-builder" >&2; exit 1; }
[ -x target/dist/subcraft ] || { echo "target/dist/subcraft is missing: build it first (cargo build --locked --profile dist -p subcraft)" >&2; exit 1; }

case "$(uname -m)" in
    x86_64|amd64) arch=x86_64 ;;
    aarch64|arm64) arch=aarch64 ;;
    *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

# Stage the files the manifest installs.
src=target/flatpak/src
rm -rf target/flatpak
mkdir -p "$src/third-party" dist
cp target/dist/subcraft "$src/subcraft"
sed "s/^Icon=.*/Icon=$app_id/" packaging/linux/subcraft.desktop > "$src/subcraft.desktop"
cp packaging/linux/subcraft.png "$src/subcraft.png"
sed -e "s/@VERSION@/$version/" -e "s/@DATE@/$(date -u +%Y-%m-%d)/" \
    "packaging/linux/flatpak/$app_id.metainfo.xml" > "$src/$app_id.metainfo.xml"
cp README.md LICENSE-MIT LICENSE-APACHE "$src/"
cp crates/ui/assets/fonts/*-LICENSE.txt "$src/third-party/"
cp assets/starter-pack/CREDITS-CC0.txt "$src/third-party/starter-pack-CC0.txt"
cp assets/environments/CREDITS-CC0.txt "$src/third-party/environments-CC0.txt"
cp assets/open-pack/CREDITS-CC0.txt "$src/third-party/open-pack-CC0.txt"

out="dist/SubCraft-$version-linux-$arch.flatpak"
rm -f "$out"
# $builder is split on spaces on purpose (it may be a "flatpak run ..." command).
# shellcheck disable=SC2086
$builder --user --install-deps-from=flathub --force-clean --disable-rofiles-fuse \
    --repo=target/flatpak/repo target/flatpak/build "packaging/linux/flatpak/$app_id.yml"
flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
    target/flatpak/repo "$out" "$app_id"
echo "Done: $out ($(du -h "$out" | cut -f1))"
