# Handoff — natural Edge Wear and actual-crate joins (2026-10-19)

- Branch feature/brush-roadmap; implementation commit **a1c8446**. Final docs commit is HEAD.
- cargo test --workspace: **468 pass, 0 failed/ignored** (8 added). Workspace/all-target Clippy
  exits 0 with existing warnings. CARGO_INCREMENTAL=0, dev debug=line-tables-only, -j 1.
- Done: independent convex-crease source fixes the default crate's body/post contact gaps.
- Done: Natural edge wear opt-in/default for new Edges, Spread, Variation, local Level reach,
  irregular boundary at Breakup 0; stronger useful noise range verified in final captures.
- Done: shared topology across sets/resolutions, tolerance-aware seam matching, bounded cached
  distance fields, conservative connected paths across bends; no UV/scene-space dilation.
- Done: schema **8**; older project/recipe generators missing edge_spread retain legacy output.
  Controls use the existing undoable SetMaskEffect, saved in projects/smart recipes.
- Done: actual-crate regression, numeric planar/subdivision/seam/thin/isolation/output tests,
  seed/size/cache checks, undo/reopen/latest-worker convergence, migration and UI opt-in/undo.

Updated app: **E:/Applications/ArtCraft/SubCraft/target/debug/subcraft.exe**. Restart this
binary; existing masks need **Natural edge wear** enabled. Prior latest/subcraft.exe retained.
Native captures inspected: docs/screenshots/edge-wear-natural-crate.png and
edge-wear-natural-crate-wide.png (actual crate, both sets, baked Curvature 512, Breakup 0,
Variation 0.85, Spread 2.5/4.5%). Isolated data/library, exit 0, no physical mouse driven.
No push/release. Repro fixture/profile: app/examples/edge_spread.rs; uses desktop mimalloc.

4K CPU report: docs/performance/2026-10-19-natural-edge-spread-4k.txt. Cold 2687.8 ms;
edit/poll p50 0.398/p95 0.541/max 1.059 ms; first edit to settle 708.3 ms, 41 frames. Both sets,
geometric sources without baked maps, optimized dev build. A preliminary System-heap harness
attempt was stopped/excluded before matching the app allocator; not an app performance result.
No GPU/upload/render/long-stroke frame-pacing claim. No known failing/skipped tests.

Partial: materials/sample/edge_distance.rs::SurfaceEdges::distance is exact on convex planar
patches, conservative vertex paths elsewhere; sources are manifold convex creases >20 degrees.
compositor/mask.rs::eval_edge_spread retains baked positive detail but does not propagate its
independent distance outward. Baked-only/painted-normal detail expansion, Dirt spread and
strict lower-layer normal stages/Disable Micro Effects remain pending. See ADR-0027.
Physical gestures/tablet, alternate DPI/browser and production imported/high-poly models were
not hand-verified. Session 82's isolated bake fix alone did not resolve crate contact gaps.

Unrelated working changes preserved: Cargo.lock, crates/ui/Cargo.toml, crates/ui/examples,
artworks/Dead-Man. Protected zips/output/generate_textures.py/__pycache__ untouched. Earlier
recursive cache cleanup was blocked by approval review; never bypass with another delete tool.

Next 3 steps:
1. Owner opts the existing crate mask into Natural edge wear and adjusts Spread/Variation.
2. Extend source/stage contracts for baked-only detail and Dirt; improve curved-surface distance.
3. Add strict lower-layer normal inputs/Disable Micro Effects, then neighborhood filters and
   complete 4K upload/render profiling on production assets.
