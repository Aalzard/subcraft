# Handoff — bottom panels, cleanup audit and next artwork (2026-10-19)

- Branch feature/brush-roadmap; code commit d23ddee. Docs are the final commit (git log -1).
- cargo test --workspace: 457 pass, 0 failed/ignored (3 added). Workspace/all-target Clippy
  exits 0 with existing warnings. CARGO_INCREMENTAL=0, dev debug=line-tables-only, -j 1.
- Done: default Shelf-left layout, Parts/Texture Sets/History bottom-center; dock storage v5
  applies it once and later rearrangements persist. View > Reset layout restores it.
- Done: Texture Sets uses horizontal selection / resolution-filter / bake-mesh regions at
  widths >=660 points. Narrow sidebars stay vertical. Common command helpers retain behavior.
- Done: six unreferenced screenshots removed through Git, 1,049,698 bytes (about 1 MiB).
- Latest binary: E:/Applications/ArtCraft/SubCraft/target/debug/latest/subcraft.exe. The owner's
  normal debug/subcraft.exe was running and was not stopped/overwritten. No push or release.

Native capture inspected: docs/screenshots/bottom-texture-sets-layout.png. --demo --shelf-panel
--screenshot; isolated data/library %TEMP%/subcraft-bottom-panel-native. Body/Trim and all
compact controls fit without full-panel scrolling. Physical gestures, other DPI/monitors,
browser and 4K frame pacing were not hand-tested. Automated selection/bake/resolution/filter
and layout checks pass. No known failing/skipped tests. Docs ADR-0025 records the layout.

Cleanup BLOCKED: target totals ~16.3 GB; scoped target/release (~4.8 GB) and debug/incremental
(~1.15 GB) deletion was rejected by automatic approval review: "blocked by policy". No caches
or executables were deleted; do not retry via a different deletion route. Major E: cleanup
remains undone. Protected zips/output/generate_textures.py/__pycache__, source, artwork and
README-linked screenshots were preserved. Current dev dependencies remain for artwork work.

New owner task (create separate local SubCraft chat after this commit): fully texture
C:/Users/Rayan/Downloads/dead_man.fbx, preserving source. Reference image:
C:/Users/Rayan/AppData/Local/Temp/codex-clipboard-22c6ae2d-0e9a-423f-b48d-65de2ab8a6df.png.
Copy the reference into the new artwork folder. Pale mottled decayed skin, red exposed/wounded
tissue, dark crevices, aged teeth, varied dry/wet roughness and restrained pore/normal detail.
Import, inspect UVs/material slots, bake, author editable PBR layers/masks, save a .subcraft,
export textures and inspect front/back/detail views in the actual app. Do not overwrite the
owner's open session. Prefer supported Session/CLI workflows. Artwork is NOT done this session.

Keep schema 7 and ADR-0022/23/24 rules; no schema change this session. UI preview sizes are
preferences (88/36 defaults), caches stay bounded and independent thumbnails select domains.
Older pending feature scope remains in material-workflow-roadmap.md: actual surface-distance
wear/dirt spread; strict lower-layer normal dependencies/Disable Micro Effects; paint-channel
visibility; emission glow/glare/shine. mask.rs::eval_generator, tile.rs::apply_layer and
viewport.wgsl::fs_main retain those partial stops. Never read future/final normals for masks.
Prior debug exact 4K composites can take 14–28 seconds; no release frame-speed claim.

Next 3 steps:
1. In the new artwork task, inspect the FBX/reference, save a copied working asset and bake.
2. Build the complete editable creature material, reopen/export and inspect native renders.
3. Deliver project/maps/screenshots for owner review; then resume pending material features.
