# Codex handoff - 2026-10-19, session 88

Branch: feature/brush-roadmap. Last implementation: 62af124; documentation commit follows.
519 workspace tests PASS, 0 failed/ignored (10 new); Clippy workspace/all-targets exits 0,
repository warnings only. CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=line-tables-only, -j 1.
Test build: E:/Applications/ArtCraft/SubCraft/target/debug/subcraft.exe. No push/release.

Done:
- Bevel: thresholded inward ramp, radius/amount; negative signed Height gives recessed relief.
- Mask Outline: threshold/radius, Inner/Outer/Both; same content/mask/folder stacks and commands.
- Bounded linear-time scalar Euclidean distance transform; no full-layer dense image.
- Preview scaling, cache/dirty halos, latest worker, one-gesture undo and exact export retained.
- Schema 12 / legacy 1-11 identity migration, actual schema-11 stack test and smart round-trips.
- Independent brute-force kernel/tiled goldens, alpha/opacity, cache, invalid edits and UI checks.
- Native crate captures: docs/screenshots/filter-{bevel,outline,mask-bevel,mask-outline,outline-2d}.png.
- CPU 4K report: docs/performance/2026-10-19-bevel-outline-4k.txt; Bevel poll p95 .926 / max 4.938ms,
  converge .578s; Outline p95 .696 / max .711ms, converge .642s. CPU only, not desktop FPS.

No partial contour feature remains. Extension points: materials/src/filter/contour.rs and
compositor/src/stages.rs::Graph::neighborhood, Node::Contour. Keep ADR-0032: previous-stage only,
threshold raw scalar or linear luminance multiplied by coverage; transparent always outside.
Distance is exact between centers, contour approximated minus half a pixel. No invented border
outside texture. Uniform white Bevel = plateau, Outline = zero. Radius zero: Bevel hard plateau,
Outline zero (neither identity). Both radius is PER SIDE. Preserve content alpha; outward
material growth belongs on masks. Height signed, other outputs bounded. Halo ceil(radius+.5),
preview radius scaling, bounded cache, source immutability, sync parity and schema migration.

Known: owner-rejected Edge Wear unresolved, kernels untouched; Claude prompt still applies.
No failing/skipped tests. UV filters mix atlas islands and do not follow mesh seams. No physical
mouse driven. Manual drags/tablet, browser/DPI, production assets and full 4K GPU/upload/
long-stroke frame pacing NOT verified. Open: atlas-aware policy and future Normal filter semantics.
Bevel Smooth/Height Adjust, effect thumbnails/drag reorder, GPU parity, references, lower-normal
Dirt and emission remain unimplemented. Coordinate shared files if Claude edits wear concurrently.
Preserve unrelated Cargo.lock, UI Cargo.toml/examples, artworks/Dead-Man; protected zips/output/
generate_textures.py/__pycache__ untouched. Earlier cleanup was approval-rejected; do not bypass.

Next 3 steps:
1. Add per-effect thumbnails and drag reorder while preserving independent domain selection.
2. Bevel Smooth/Height Adjust with numeric/native gates on the existing neighborhood contract.
3. Profile full 4K desktop upload/render/long strokes, then GPU parity and lower-normal references.
