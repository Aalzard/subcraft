#!/usr/bin/env bash
# Build the SubCraft MCP server and install it as /usr/local/bin/subcraft-mcp.
# Meant for WSL (Windows with Smart App Control blocks locally built .exe files) or Linux.
# Run from Windows:  wsl -d Ubuntu -e bash /mnt/d/subcraft/scripts/build-mcp-wsl.sh
set -euo pipefail
. "$HOME/.cargo/env" 2>/dev/null || true
command -v cargo >/dev/null || { echo "cargo not found: install Rust with https://rustup.rs" >&2; exit 1; }
here="$(cd "$(dirname "$0")/.." && pwd)"
# Keep build output on the Linux disk: building on /mnt/<drive> is several times slower.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/subcraft-target}"
cd "$here"
cargo build --release -p subcraft-mcp
cargo test --release -p subcraft-mcp
bin="$CARGO_TARGET_DIR/release/subcraft-mcp"
if [ "$(id -u)" -eq 0 ]; then install -m 755 "$bin" /usr/local/bin/subcraft-mcp; else sudo install -m 755 "$bin" /usr/local/bin/subcraft-mcp; fi
echo "installed /usr/local/bin/subcraft-mcp (restart Claude Code or run /mcp to reconnect)"
