# Painter `scene.assbin` (format 1.7)

Substance Painter stores the project mesh in HDF5 at `paint/scene.assbin`. Public Assimp 6 only reads assbin **1.0**; Painter 11 ships a forked `assimp.dll` that reads **1.7**.

## SubCraft module: `subcraft-assbin-host`

- Crate: `crates/assbin-host`
- Loads Adobe `assimp.dll` at runtime (not linked, not redistributed).
- Resolve order: `SUBCRAFT_PAINTER_ASSIMP` → default Painter install path.
- Before `LoadLibrary`, working directory is set to the DLL directory (required for dependencies).

Export for mesh import tries, in order: `glb2` (glTF 2), `obj`, legacy `glb` (glTF 1 — not loaded by `subcraft-mesh`).

Wiring:

- `subcraft-spp::try_assbin_to_glb` / `open_pack` when `SUBCRAFT_SPP_ASSIMP=1` or Painter DLL is present.
- `subcraft-mesh::import_assbin_bytes` uses the host on Windows before the in-tree Rust decoder.

CLI: `cargo build -p painter-assimp-export --release` → `target/release/painter-assimp-export.exe`.

## Pure-Rust decoder

`crates/mesh/src/assbin/painter_blob.rs` — scene-graph tail @ **4257**, **11** mesh blobs, `nf` face count @ header+12, `nv = (geom − nf×12) / 36`, vertices (pos/normal/uv₃), faces as **`nf × (3×u32)`** indices. API: `subcraft_mesh::import_assbin_pure` (tests on `Drehgestell.spp`). Default `import_assbin_bytes` uses the Rust decoder only (no Substance Painter install). Optional regression: `SUBCRAFT_USE_PAINTER_ASSIMP=1` with `cargo build -p subcraft-mesh --features painter-assimp-host`. Scene-graph **instance transforms** not applied yet (meshes merged at origin). See `docs/painter-re-findings.md`.

## Tests

With `Drehgestell.spp` and Painter installed:

```text
cargo test -p subcraft-mesh import_drehgestell_assbin_via_host
cargo test -p subcraft-spp painter_dll_exports
```

Optional: `SUBCRAFT_SPP_FIXTURE=path\to\file.spp`
