# Codex handoff - 2026-10-20, session 90

Branch: feature/brush-roadmap. Last implementation: a995822; documentation commit follows.
538 workspace tests PASS, 0 failed/ignored (10 new); Clippy workspace/all-targets exits 0,
repository warnings only. CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=line-tables-only, -j 1.
Test build: E:/Applications/ArtCraft/SubCraft/target/debug/subcraft.exe. No push/release.

Done:
- Drag effect names/rows/preview squares within their layer and content/mask domain.
- Amber insertion line, Escape/outside cancellation, no-op rejection, typed payloads, edge scroll.
- App resolves live UUID anchors/index after removal; one existing Move command + SelectEffect.
- One undo, retained raster Arcs/IDs/settings, moved-effect Properties/paint routing, schema 12.
- Exhaustive indices, numeric order/preview/export/reopen and real headless pointer-event tests.
- Native post-drop captures: docs/screenshots/effect-reorder-{content,mask}.png, inspected.
- CPU 4K report: docs/performance/2026-10-20-effect-reorder-4k.txt; 18 previews/eight drop rounds,
  poll p95 .133 / max .166ms, settled .422s. Full composite/GPU/render/upload excluded.

No partial drag work remains. Keep ADR-0034: app/src/effect_stack.rs::drop_destination validates
same texture set/layer/domain and UUIDs, resolves bottom-to-top AFTER removal, returns None
for stale/self/unchanged drops. session.rs::DropEffect rejects changed active set, invokes one
MoveContentEffect/MoveMaskEffect then SelectEffect. UI only maps coordinates/commands; typed
release guards must preserve shelf/layer payloads. Preview squares remain independently clickable.
Existing context-menu moves retained. Order uses existing project fields, no migration needed.
Cross-layer/domain transfer is outside this slice. Preserve ADR-0033 stage-preview semantics.

Known: no failing/skipped tests; native captures show controller-generated POST-drop order.
Actual drag/marker/Escape/scroll behavior exercised in headless egui tests only. No physical
mouse driven; native mouse/tablet and mid-drag marker, browser/DPI, production scenes and full
4K desktop/frame pacing NOT hand-verified. 64px fine-detail preview limits remain. CPU poll
numbers are not FPS. Owner-rejected Edge Wear unresolved; kernels untouched/Claude prompt applies.
Preserve unrelated Cargo.lock, UI Cargo.toml/examples, artworks/Dead-Man; protected zips/output/
generate_textures.py/__pycache__ untouched. Earlier cleanup was approval-rejected; do not bypass.

Next 3 steps:
1. Bevel Smooth/Height Adjust with existing stage contracts and numeric/native gates.
2. Full 4K desktop upload/render/long-stroke profiling; measured GPU filter parity.
3. Compare Mask/anchor references and lower-normal Dirt dependencies, then emission workflow.
