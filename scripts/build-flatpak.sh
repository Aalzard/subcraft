#!/bin/sh
# Package SubCraft as dist/SubCraft-<version>-linux-<arch>.flatpak, a single-file bundle that
# `flatpak install --user <file>` installs (it pulls the freedesktop runtime from Flathub).
# Needs target/dist/subcraft (cargo build --profile dist -p subcraft, or ./scripts/build-linux.sh),
# flatpak, and the runtime and SDK it builds against:
#   flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
#   flatpak install --user flathub org.freedesktop.Platform//26.08 org.freedesktop.Sdk//26.08
# The app is a prebuilt binary plus data files, so plain `flatpak build-*` commands assemble it (no
# flatpak-builder). Keep the permissions below in step with
# packaging/linux/flatpak/io.github.aalzard.SubCraft.yml, the manifest for flatpak-builder / Flathub.
# Usage: ./scripts/build-flatpak.sh
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
app_id=io.github.aalzard.SubCraft
branch=26.08
version="$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' Cargo.toml | head -n 1)"
[ -n "$version" ] || { echo "could not read the version from Cargo.toml" >&2; exit 1; }
command -v flatpak >/dev/null || { echo "flatpak is missing: install it" >&2; exit 1; }
[ -x target/dist/subcraft ] || { echo "target/dist/subcraft is missing: build it first (cargo build --locked --profile dist -p subcraft)" >&2; exit 1; }

case "$(uname -m)" in
    x86_64|amd64) arch=x86_64 ;;
    aarch64|arm64) arch=aarch64 ;;
    *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

build=target/flatpak/build
repo=target/flatpak/repo
rm -rf target/flatpak
mkdir -p dist
flatpak build-init "$build" "$app_id" org.freedesktop.Sdk org.freedesktop.Platform "$branch"

# /app inside the sandbox.
app="$build/files"
install -Dm755 target/dist/subcraft "$app/bin/subcraft"
mkdir -p "$app/share/applications"
sed "s/^Icon=.*/Icon=$app_id/" packaging/linux/subcraft.desktop > "$app/share/applications/$app_id.desktop"
install -Dm644 packaging/linux/subcraft.png "$app/share/icons/hicolor/256x256/apps/$app_id.png"
mkdir -p "$app/share/metainfo"
sed -e "s/@VERSION@/$version/" -e "s/@DATE@/$(date -u +%Y-%m-%d)/" \
    "packaging/linux/flatpak/$app_id.metainfo.xml" > "$app/share/metainfo/$app_id.metainfo.xml"
install -Dm644 -t "$app/share/doc/subcraft" README.md LICENSE-MIT LICENSE-APACHE
install -Dm644 -t "$app/share/doc/subcraft/third-party" crates/ui/assets/fonts/*-LICENSE.txt
install -Dm644 assets/starter-pack/CREDITS-CC0.txt "$app/share/doc/subcraft/third-party/starter-pack-CC0.txt"
install -Dm644 assets/environments/CREDITS-CC0.txt "$app/share/doc/subcraft/third-party/environments-CC0.txt"
install -Dm644 assets/open-pack/CREDITS-CC0.txt "$app/share/doc/subcraft/third-party/open-pack-CC0.txt"

flatpak build-finish "$build" --command=subcraft \
    --share=ipc --socket=wayland --socket=fallback-x11 --device=dri --share=network --filesystem=home
flatpak build-export "$repo" "$build"
out="dist/SubCraft-$version-linux-$arch.flatpak"
rm -f "$out"
flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo "$repo" "$out" "$app_id"
echo "Done: $out ($(du -h "$out" | cut -f1))"
