# Codex handoff - 2026-10-19, session 87

Branch: feature/brush-roadmap. Last implementation: 3fc267d; documentation commit follows.
509 workspace tests PASS, 0 failed/ignored (13 new); Clippy workspace/all-targets exits 0,
repository warnings only. CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=line-tables-only, -j 1.
Test build: E:/Applications/ArtCraft/SubCraft/target/debug/subcraft.exe. No push/release.

Done:
- Directional Blur on content/masks/folders: radius, UV angle, premultiplied bilinear line sampling.
- Highpass on both domains/folders: radius/amount, original alpha, neutral .5 or signed Height 0.
- Existing undo/worker/cache/dirty halos/scaled previews reused; exact sync export and reopen tested.
- Schema 11 / legacy 1-10 migration; actual schema-10 Blur/Sharpen fixture; smart stacks round-trip.
- Dense numeric borders/angles/alpha/order/blends/folders/cache goldens; invalid edits/UI checks.
- Native crate captures: docs/screenshots/filter-{directional,highpass,mask-directional,
  mask-highpass,directional-2d}.png. CLI exit 0; inspected results/properties/thumbnails.
- CPU 4K report: docs/performance/2026-10-19-directional-highpass-4k.txt. Directional poll p95
  1.071 / max 9.492ms, settle 3.539s; Highpass p95 1.351 / max 2.303ms, settle 1.316s.

No partial filter remains. Kernel extension point: compositor/src/stages.rs::Graph::neighborhood,
Node::Directional/Gaussian and materials/src/filter.rs. UI only dispatches existing effect commands.
Keep ADR-0031: angle follows top-left UV (+45 down-right), 180-degree periodic; premultiplied
bilinear samples; Highpass radius/amount zero = neutral, never identity. Known-uniform lowpass
inputs bypass convolution. Height stays signed; other Highpass/HDR values clamp to 0-1. Normal
vectors unsupported. Preserve previous-stage data, cache bounds, sync parity and schema migrations.

Known: rejected Edge Wear is unresolved; generator kernels untouched; Claude prompt still applies.
No failing/skipped tests. Directional settlement is more expensive; further optimization remains.
UV filters can mix atlas islands and do not follow mesh seams. No physical mouse driven. Manual
drags/tablet, browser/DPI, production assets, long strokes and full 4K GPU/upload pacing NOT verified.
CPU polling is not FPS. Open decisions: atlas-aware policy and future Normal filter semantics.
Bevel/Mask Outline, effect thumbnails/drag reorder, GPU parity, references and lower-normal Dirt
remain unimplemented (not half-written). Coordinate shared files if Claude edits wear concurrently.

Preserved unrelated Cargo.lock, UI Cargo.toml/examples, artworks/Dead-Man; protected zips/output/
generate_textures.py/__pycache__ untouched. Earlier cleanup was approval-rejected; do not bypass.

Next 3 steps:
1. Implement Bevel/Mask Outline on the existing neighborhood contract, with numeric/native gates.
2. Add per-effect thumbnails and drag reorder without changing content/mask target selection.
3. Profile full 4K desktop uploads/render/long strokes, then GPU parity and lower-normal references.
