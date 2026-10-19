# Codex handoff - 2026-10-19

Branch: feature/brush-roadmap. Last implementation commit: 187e3ca; docs commit follows.
496 workspace tests PASS, 0 failed/ignored; Clippy workspace/all-targets exits 0 (repository warnings).
Use CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=line-tables-only, -j 1 before each commit.
Test executable: E:/Applications/ArtCraft/SubCraft/target/debug/subcraft.exe. No push/release.

Done:
- Blur/Sharpen on content/masks/folders; Radius/Amount controls; existing editing/undo reused.
- Previous-stage DAG, separable Gaussian, 64 MiB/set completed tile cache + active frontier;
  uniform-stage sharing, absent-source Replace parity, conservative chained raster halos.
- Premultiplied Blur coverage, Sharpen alpha/signed Height, scaled previews, schema 10 / v1-9 migration.
- Numeric dense references, parallel eviction/all blends, latest worker/export/undo/reopen and UI tests.
- Native crate 3D/2D/mask captures: docs/screenshots/filter-{blur,sharpen,mask-blur,blur-2d}.png.
- 4K CPU report: docs/performance/2026-10-19-neighborhood-filters-4k.txt. Blur convergence 8.664s
  before uniform sharing -> 1.183s; polling p95 0.892ms. Sharpen convergence 0.718s / p95 0.888ms.

No half-implemented filter remains. Next kernel extension point: compositor/src/stages.rs::Graph::gaussian
and Node::Gaussian evaluation; doc/src/filter.rs holds parameters, UI dispatches existing commands.
Keep exact sync/worker parity, bounded immutable stages, previous-stage ordering and schema migrations.
ADR-0030 records UV edge clamping, radius units, alpha and caching. UV filters can mix nearby atlas
islands and do not follow mesh seams. Normal filters unsupported; Sharpen clips non-Height/HDR to 0-1.

Known unresolved: owner REJECTS Edge Wear (docs/PROMPT-claude-edge-wear.md). Do not claim it fixed;
mask.rs::evaluate_effect is now shared by local and spatial evaluators; coordinate Claude edits.
No failing/skipped tests. No physical mouse driven; manual drags/tablet, production assets, browser/DPI,
full 4K GPU/upload/frame pacing and long strokes NOT hand-verified. CPU timings do not establish FPS.
GPU filter backend, remaining filters, references/anchors and lower-normal-aware Dirt remain pending.
Open owner decisions: future island-aware UV policy and Normal-filter semantics.

Unrelated Cargo.lock, crates/ui/Cargo.toml, UI examples and artworks/Dead-Man retained. Protected zips,
output/, generate_textures.py and __pycache__/ untouched. Earlier cleanup was approval-rejected; do not bypass.

Next 3 concrete steps:
1. Claude reproduces unresolved wear on the default baked crate using the owner handoff prompt.
2. Extend Graph with Directional Blur/Highpass, goldens/migration/native evidence; then effect thumbnails.
3. Profile full 4K desktop GPU/upload/long-stroke pacing before lower-normal-aware Dirt and references.
