#!/bin/sh
# Install SubCraft for the current user (no root needed): the program goes to ~/.local/bin and a
# launcher to your application menu. Run again to update; ./install.sh --uninstall removes it.
set -e
here="$(cd "$(dirname "$0")" && pwd)"
bin="$HOME/.local/bin"
apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
icons="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/256x256/apps"

if [ "$1" = "--uninstall" ]; then
    rm -f "$bin/subcraft" "$apps/subcraft.desktop" "$icons/subcraft.png"
    echo "SubCraft removed."
    exit 0
fi

mkdir -p "$bin" "$apps" "$icons"
install -m 755 "$here/subcraft" "$bin/subcraft"
install -m 644 "$here/subcraft.png" "$icons/subcraft.png"
sed "s|^Exec=subcraft|Exec=$bin/subcraft|" "$here/subcraft.desktop" > "$apps/subcraft.desktop"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps" >/dev/null 2>&1 || true
echo "SubCraft installed. Start it from your application menu, or run: $bin/subcraft"
