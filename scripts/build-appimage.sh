#!/bin/sh
# Package SubCraft as dist/SubCraft-<version>-linux-<arch>.AppImage: one file that runs on most
# Linux desktops without installing (chmod +x, then double-click or run it).
# Needs target/dist/subcraft (cargo build --profile dist -p subcraft, or ./scripts/build-linux.sh).
# Usage: ./scripts/build-appimage.sh
# APPIMAGETOOL=/path/to/appimagetool uses that tool; otherwise a pinned release is downloaded
# into target/appimage-tools and checked against its SHA-256 (x86_64 only).
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
version="$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' Cargo.toml | head -n 1)"
[ -n "$version" ] || { echo "could not read the version from Cargo.toml" >&2; exit 1; }
[ -x target/dist/subcraft ] || { echo "target/dist/subcraft is missing: build it first (cargo build --locked --profile dist -p subcraft)" >&2; exit 1; }

case "$(uname -m)" in
    x86_64|amd64) arch=x86_64 ;;
    aarch64|arm64) arch=aarch64 ;;
    *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

# appimagetool 1.9.0 (https://github.com/AppImage/appimagetool/releases/tag/1.9.0).
tool_version=1.9.0
tool_sha256_x86_64=46fdd785094c7f6e545b61afcfb0f3d98d8eab243f644b4b17698c01d06083d1
tool="${APPIMAGETOOL:-}"
if [ -z "$tool" ]; then
    [ "$arch" = x86_64 ] || { echo "set APPIMAGETOOL to an appimagetool for $arch" >&2; exit 1; }
    tool="target/appimage-tools/appimagetool-$tool_version-$arch"
    if [ ! -x "$tool" ]; then
        mkdir -p target/appimage-tools
        curl -fsSL -o "$tool.part" "https://github.com/AppImage/appimagetool/releases/download/$tool_version/appimagetool-$arch.AppImage"
        echo "$tool_sha256_x86_64  $tool.part" | sha256sum -c - >/dev/null || { rm -f "$tool.part"; echo "appimagetool checksum mismatch" >&2; exit 1; }
        chmod +x "$tool.part"
        mv "$tool.part" "$tool"
    fi
fi

name="SubCraft-$version-linux-$arch"
appdir="dist/$name.AppDir"
rm -rf "$appdir" "dist/$name.AppImage"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" "$appdir/usr/share/icons/hicolor/256x256/apps" "$appdir/usr/share/doc/subcraft/third-party"
cp target/dist/subcraft "$appdir/usr/bin/subcraft"
chmod +x "$appdir/usr/bin/subcraft"
cp packaging/linux/subcraft.desktop "$appdir/subcraft.desktop"
cp packaging/linux/subcraft.desktop "$appdir/usr/share/applications/subcraft.desktop"
cp packaging/linux/subcraft.png "$appdir/subcraft.png"
cp packaging/linux/subcraft.png "$appdir/usr/share/icons/hicolor/256x256/apps/subcraft.png"
ln -s subcraft.png "$appdir/.DirIcon"
cp README.md LICENSE-MIT LICENSE-APACHE "$appdir/usr/share/doc/subcraft/"
cp crates/ui/assets/fonts/*-LICENSE.txt "$appdir/usr/share/doc/subcraft/third-party/"
cp assets/starter-pack/CREDITS-CC0.txt "$appdir/usr/share/doc/subcraft/third-party/starter-pack-CC0.txt"
cp assets/environments/CREDITS-CC0.txt "$appdir/usr/share/doc/subcraft/third-party/environments-CC0.txt"
cp assets/open-pack/CREDITS-CC0.txt "$appdir/usr/share/doc/subcraft/third-party/open-pack-CC0.txt"
cat > "$appdir/AppRun" <<'EOF'
#!/bin/sh
# Start SubCraft from inside the AppImage, passing the arguments on.
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/bin/subcraft" "$@"
EOF
chmod +x "$appdir/AppRun"

# The tool is itself an AppImage: extract-and-run works where FUSE is missing (containers, CI).
APPIMAGE_EXTRACT_AND_RUN=1 ARCH="$arch" "$tool" --no-appstream "$appdir" "dist/$name.AppImage"
rm -rf "$appdir"
chmod +x "dist/$name.AppImage"
echo "Done: dist/$name.AppImage ($(du -h "dist/$name.AppImage" | cut -f1))"
