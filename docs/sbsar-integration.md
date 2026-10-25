# Optional Substance `.sbsar` support

SubCraft can bake Adobe Substance Designer/Painter `.sbsar` graphs into texture maps and apply them as fill layers. **No Adobe binaries or sample `.sbsar` files are shipped in this repository.** You bring your own SDK and materials.

## What you need

| Piece | Where to get it |
|-------|-----------------|
| **Substance 3D Materials SDK** (Windows x64) | [Adobe Substance 3D Developer](https://substance3d.adobe.com/documentation/) / Adobe Developer portal — download the Materials SDK, unpack locally (e.g. `_sdk_extract/substance-win-x64-…`). Do **not** commit the SDK. |
| **Bake host** (`subcraft-sbsar-bake.exe`) | Built from this repo: `native/sbsar-bake` via `.\scripts\build-sbsar-host.ps1` (links against the SDK). Output: `bin/sbsar-bake/` (gitignored). |
| **`.sbsar` materials** | Your own Designer/Painter exports; Substance 3D Assets / Source (respect Adobe’s license); anything you already own. Import via **Shelf → Materials → Import SBSAR…** or drop files into `Documents/SubCraft/Library/sbsar/`. |

Environment overrides (optional):

- `SUBSTANCE_SDK_ROOT` — SDK root if not under `_sdk_extract`
- `SUBSTANCE_SDK_BIN` — folder with `substance_sse2_blend.dll`
- `SUBCRAFT_SBSAR_HOST` — full path to the bake EXE if not next to `subcraft.exe` / under `bin/sbsar-bake`
- `SUBCRAFT_LIBRARY` — user library root (default `Documents/SubCraft/Library`)

## Build the host

```powershell
.\scripts\build-sbsar-host.ps1
```

The host writes per-output `.tga` files plus `manifest.json`. SubCraft maps filenames like a normal texture set (`basecolor`, `roughness`, `normal`, …).

## In the app

1. Build SubCraft (`cargo run -p subcraft --release`).
2. Ensure the bake host is on the search path (script output or `SUBCRAFT_SBSAR_HOST`).
3. **Shelf → Materials → Import SBSAR…** (or **Layer → Import SBSAR…**) — files are copied into the user library.
4. Click or drag a tile onto the model / a layer — bake runs at the **Apply size** chosen in the Materials toolbar (preview stays 512²).
5. Imported fill layers keep live graph parameters in **Properties** (rebake when you change them).

## Architecture

1. `native/sbsar-bake` — C++ CLI against `substance_framework`
2. `crates/sbsar` — locate host, spawn process, read manifest (pure Rust; compiles without the SDK)
3. `crates/app` (`sbsar` / `sbsar_fill`) — bake into a temp dir, then `import_texture_set` / fill-layer helpers
4. `crates/ui` (`sbsar_materials`) — shelf toolbar + grid; `shelf.rs` only calls thin hooks

## Painter Smart Materials / Masks (`.spsm` / `.spmsk`)

No Painter assets are bundled. Import your own files, or point SubCraft at an installed Painter shelf:

| Need | Typical location (Windows) |
|------|----------------------------|
| Starter smart materials | `C:\Program Files\Adobe\Adobe Substance 3D Painter\resources\starter_assets\smart-materials\` |
| Starter smart masks | `…\starter_assets\smart-masks\` |
| Procedural noises (Ratio Cells, …) | `…\starter_assets\procedurals\` (override with `SUBCRAFT_PAINTER_PROCEDURALS`) |

Shelf → Smart Materials / Smart Masks → **Import…** or **Load Painter Starter…** when those folders exist. On apply, SubCraft re-decodes `.spsm`/`.spmsk` and bakes Painter noise `.sbsar` graphs when the SBSAR host (above) and procedurals folder are available; otherwise it falls back to Pattern approximations.

See also [ADR 0007](adr/0007-fork-upstream-sbsar.md) for merge boundaries.
