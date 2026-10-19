# Codex handoff - 2026-10-20, session 89

Branch: feature/brush-roadmap. Last implementation: c88079a; documentation commit follows.
528 workspace tests PASS, 0 failed/ignored (9 new); Clippy workspace/all-targets exits 0,
repository warnings only. CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=line-tables-only, -j 1.
Test build: E:/Applications/ArtCraft/SubCraft/target/debug/subcraft.exe. No push/release.

Done:
- Inclusive stage thumbnails in content/mask rows, own channel for Filters/Levels, raw masks.
- Click square selects effect without toggling/history; type icons, compact rows, disabled mark.
- Existing Preview size footer scales effect squares 18-28px; visible/collapsed request policy.
- Existing bounded single-worker/LRU/stale rejection; removed UUID handling and transient keys.
- Correct folder inputs: passthrough lower accumulation, ancestor isolation, no later stages.
- Numeric/lifecycle/UI/cache tests and 4K Paint-effect stroke with one undo; schema remains 12.
- Native screenshots: docs/screenshots/effect-thumbnails-{content,mask,folder}.png, inspected.
- 4K CPU report: docs/performance/2026-10-20-effect-thumbnails-4k.txt; 18 previews, eight revisions,
  request/poll p95 .128 / max .202ms, final settlement .425s. Excludes composite/GPU/render/upload.
- Separate 4K Paint-effect stroke CPU test: cold 5.456ms, warm p95 1.565 / max 1.643ms; one undo.

No partial thumbnail work remains. Keep ADR-0033: PreviewKey.effect is inclusive UUID stage,
worker-only COW truncation. Disabled effects show bypassed result; disabled parent layer/mask
still permits authored inspection. Preserve data/color encoding, paint alpha and scaled radii.
Compositor before()/content_preview_in_stack handle folder ancestor inputs; app snapshots root
Arc pointers for folder invalidation. UI never evaluates/truncates document effects. No schema
bump needed for transient images. CPU/GPU cache limits shared at 96; request only visible rows.
Extension points: app/src/layer_previews.rs::evaluate/request; compositor/src/stages.rs::before;
ui/src/panels/effects.rs::rows. Existing context-menu reorder remains; drag reorder NOT added.

Known: fine details can vanish at 64px; prefixes currently evaluate independently. No failing/
skipped tests. No physical mouse driven; native clicks/drags/tablet, browser/DPI, production
models and full 4K GPU/upload/long-stroke FRAME pacing not hand-verified. CPU stroke timing is
not FPS. Owner-rejected Edge Wear unresolved, kernels untouched; Claude prompt still applies.
GPU filters, Bevel Smooth/Height Adjust, references/lower-normal Dirt and emission remain open.
Preserve unrelated Cargo.lock, UI Cargo.toml/examples, artworks/Dead-Man; protected zips/output/
generate_textures.py/__pycache__ untouched. Earlier cleanup was approval-rejected; do not bypass.

Next 3 steps:
1. Effect-row drag reorder with correct domain selection, undo and native checks.
2. Bevel Smooth/Height Adjust on the existing filter stages with numeric/native gates.
3. Full 4K desktop upload/render/stroke profile; then GPU parity and lower-normal references.
