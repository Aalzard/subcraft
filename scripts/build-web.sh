#!/usr/bin/env bash
# Build the web version into dist/web (or $1): index.html, icon, subcraft.js and the wasm module.
# Needs: rustup target add wasm32-unknown-unknown; cargo install wasm-bindgen-cli --version <the
# wasm-bindgen version in Cargo.lock>. Serve the folder with any static file server, e.g.
# `python -m http.server -d dist/web 8765`, and open http://localhost:8765 in a WebGPU browser.
set -euo pipefail
cd "$(dirname "$0")/.."
out="${1:-dist/web}"
target="${CARGO_TARGET_DIR:-target}"
cargo build --profile dist -p subcraft --target wasm32-unknown-unknown
mkdir -p "$out"
wasm-bindgen --target web --no-typescript --out-dir "$out" "$target/wasm32-unknown-unknown/dist/subcraft.wasm"
cp web/index.html web/icon.png "$out/"
mkdir -p "$out/starter-pack"
cp -R assets/starter-pack/textures assets/starter-pack/alphas "$out/starter-pack/"
cp assets/starter-pack/CREDITS-CC0.txt "$out/starter-pack/"
ls -l "$out"
