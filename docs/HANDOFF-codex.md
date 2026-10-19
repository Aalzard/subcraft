# Handoff — connected Edge Wear corners (2026-10-19)

- Branch feature/brush-roadmap; code commit **8dd3014**; final docs commit is git log -1.
- cargo test --workspace: **460 pass, 0 failed/ignored**, 3 added. Clippy workspace/all-targets
  exits 0 with existing warnings. CARGO_INCREMENTAL=0, dev debug=line-tables-only, -j 1.
- Done: fixed coincident curvature samples weakening at hard corners/UV rims by removing the
  centroid's along-crease direction (bake/src/curvature.rs::curvature_at; ADR-0026).
- Done: numeric corner/subdivision/concavity/scale tests, baked rim acceptance and actual Edges
  composite/save/reopen test. Regression failed before correction.
- Done: app/examples/edge_corners.rs makes an editable 512-square fixture. Native desktop
  capture inspected: docs/screenshots/edge-wear-connected-corners.png; visible gold edges join.
- Latest binary: **E:/Applications/ArtCraft/SubCraft/target/debug/subcraft.exe**. Owner's running
  target/debug/latest/subcraft.exe retained. No mouse driven, no push/release.

Not verified: owner's original affected mesh/settings, physical gestures, production high-poly
corners, alternate DPI/browser and sustained 4K painting. No known failing/skipped tests.
Rebake Curvature to update existing projects; stored maps intentionally remain unchanged on load.
Keep schema 7, signed concavity, ordinary distance falloff and existing breakup behavior.

Partial: compositor/src/mask.rs::eval_generator still thresholds baked maps; surface-distance
wear/dirt spread remains missing. compositor/src/tile.rs::apply_layer has no strict lower-normal
input stage/Disable Micro Effects. Filters/folder content effects need stage/halo contracts;
emission glow/glare/shine remains pending. See material-workflow-roadmap.md/layer-system-roadmap.md.

Unrelated working changes were present and preserved: Cargo.lock, crates/ui/Cargo.toml,
crates/ui/examples and artworks/Dead-Man (separate artwork task). Never stage them incidentally.
Protected zips/output/generate_textures.py/__pycache__ retained. Earlier recursive build-cache
cleanup was rejected by automatic approval review; do not bypass it with another deletion route.

Next 3 steps:
1. Owner checks the original corner issue after rebaking Curvature; request mesh if it persists.
2. Implement shared surface-distance/stage contract for Edge Wear/Dirt spread, with thin,
   curved/seamed mesh acceptance and bounded dirty dependencies; preserve flat faces.
3. Add strict lower-layer normal inputs and Disable Micro Effects, then neighborhood filters.
