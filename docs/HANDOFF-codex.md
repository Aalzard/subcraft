# Handoff — corner continuity and independent Spread (2026-10-19)

- Branch feature/brush-roadmap; implementation a22eb6f. Final docs commit is HEAD.
- cargo test --workspace: 470 pass, 0 failed/ignored (2 added, broader existing acceptance).
  Workspace/all-target Clippy exits 0 with warnings, including one new test-only useless_vec
  (bake/src/tests.rs bounds buffer; can use an array). Incremental=0, debug=line-tables-only, -j 1.
- Done: bake::curvature_on_surface prefers intrinsic connected-component/object curvature,
  retains flat contact concavity and uses projected high-poly triangle identity.
- Done: across-crease directions for every neighbor; centroid fallback for near-zero projected
  float error. Native inspection exposed extra edge breaks, now covered across all Body/Trim rims.
- Done: Spread independent of Level; numeric 0.065/12.5% acceptance and unchanged far flat faces.
  Level selects baked convex detail; UI tooltip/three decimals and fixture CLI updated.
- Done: Natural off/on crate output, exposed high-poly corners, absent object metadata,
  concavity/scale, undo/reopen/worker convergence. No serialized fields changed (schema 8).

App: E:/Applications/ArtCraft/SubCraft/target/debug/subcraft.exe (rebuilt and native-tested).
Restart this build and rebake Curvature for old projects. Natural masks at Level other than
0.5 intentionally change width; authored settings and stored bakes are retained (ADR-0028).
Final inspected captures (both native exit 0, baked 512, Breakup 0):
- docs/screenshots/edge-wear-baked-natural-off.png: Natural off, Level 0.85, continuous edges.
- docs/screenshots/edge-wear-low-level-wide.png: Level 0.065, Spread 12.5%, Variation 0.85.
No physical mouse driven. Manual slider/tablet and production imported high-poly models not
hand-verified; a projected ray can legitimately hit another covering part of the low mesh.

4K CPU runs retained: docs/performance/2026-10-19-independent-spread-4k.txt. Cold 2550/2607.5 ms,
poll p95 0.811/0.821 ms, max 1.676/1.342 ms, convergence 1636.5/1986.3 ms. Slower convergence
than session 83's 708.3 ms needs a controlled follow-up; no speedup/GPU/frame-pacing claim.
No known failing/skipped tests. No push/release. Previous latest/subcraft.exe remains old.

Partial stops: materials/sample/edge_distance.rs::SurfaceEdges::distance is exact on convex
planar patches, conservative vertex paths elsewhere. compositor/mask.rs::eval_edge_spread
retains baked detail but does not propagate baked-only/painted-normal distance. Dirt spread,
strict lower-layer normals/Disable Micro Effects, filters and emission effects remain pending.
Keep intrinsic-vs-contact fallback, true-distance proximity and independent Spread semantics.
Open question: preferred reach/intensity controls when extending to baked-only/normal sources.

Unrelated changes retained: Cargo.lock, crates/ui/Cargo.toml, crates/ui/examples, artworks/Dead-Man.
Protected zips/output/generate_textures.py/__pycache__ untouched. Prior recursive cache cleanup
was rejected by approval review; do not bypass through another delete route.

Next 3 steps:
1. Owner checks the saved crate after rebaking, Natural off, then 0.065/12.5% Natural wear.
2. Investigate worker convergence under controlled load and profile full 4K upload/render.
3. Extend shared distance/stage sources to baked detail/Dirt, then strict lower-layer normals.
