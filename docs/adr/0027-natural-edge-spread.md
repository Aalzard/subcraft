# ADR-0027 — Natural edge wear and connected-surface spread

Status: Accepted (2026-10-19); see session 83 in progress.md for validation and limits.

The owner's remaining gap occurs on the default crate. Its overlapping body, posts and bands
can contribute concave contact curvature beside a convex body edge. Correcting coincident
samples alone (ADR-0026) cannot prevent those contributions cancelling. A global threshold
also cannot expand a wear band predictably or create an irregular boundary at zero Breakup.

New Edges generators therefore have a separate optional `EdgeSpread` source. Shared immutable
mesh topology welds UV/hard-normal splits within each imported object, finds convex manifold
creases over 20 degrees, and measures normalized surface reach. Convex planar patches use exact
point-to-segment distance; other patches use local segments and conservative shortest paths
along mesh vertices. Convexity is checked against patch area/hull area so paths cannot cut
across holes or concave planar outlines. Vertex paths are an upper bound on geodesic distance,
not an exact continuous solver on curved surfaces. Separate objects never exchange sources.
Seam welding uses neighboring spatial buckets within one millionth of the longest extent,
capped at one percent of the shortest real edge; opposite-facing coincident sheets do not
weld. This accommodates float-rounding splits without collapsing thin connected geometry.

The topology is shared across texture sets/resolutions of a mesh. Parameter-independent tile
distance fields reuse the existing LRU/global-budget field cache; only geometry invalidates
them. Computation runs through existing immutable compositor jobs, with exact synchronous
evaluation for export/tests. This does not introduce a UV dilation, a full-resolution image
neighborhood pass, a final-normal feedback loop or app/UI dependencies in materials.

Spread is percent of the longest mesh dimension at Level 0.5. Increasing Level increases
geometric reach (twice Level), with a smooth width-controlled falloff. Volume noise changes
the reach through Variation even when Breakup is zero; Breakup adds erosion/holes. Pattern size
and Seed drive both through the existing bounded organic field. Variation zero permits a
clean geometric band. Baked positive curvature still supplies rounded/high-poly detail;
the neutral flat value is excluded. Geometric sources keep their coverage even when nearby
components weaken the bake. Expansion of baked-only detail on an otherwise smooth surface,
Dirt spread and lower-layer painted-normal inputs need a subsequent source/stage extension.

Project schema 8 adds the optional spread/variation settings. Absent settings in schemas 1–7
and older smart recipes mean the exact legacy threshold evaluator. New Edges effects enable
natural wear by default; the Properties switch explicitly opts an existing effect in/out,
through the existing undoable SetMaskEffect command. Do not silently rewrite old masks.

Acceptance must include the actual default crate joins, monotonic spread without distant flat
face coverage, UV seams and tile boundaries, zero-breakup variation, seed/size changes,
disconnected/thin geometry, migration and exact save/reopen/worker convergence. Native captures
and 4K CPU measurements must state their limits; CPU polling does not measure GPU frame time.

Session 83: 468 workspace tests pass (8 new), Clippy exits 0 with existing warnings. Native
actual-crate captures at 2.5% and 4.5% Spread/0.85 Variation/Breakup 0 are in screenshots/.
The mimalloc-matched developer harness measured 4096-square cold compositing at 2687.8 ms,
CPU edit/poll p95 0.541 ms/max 1.059 ms, and first-edit-to-settle time of 708.3 ms. See the dated performance
report; the CPU workload uses geometric sources without baked maps. GPU/render frame pacing,
long strokes, browser/DPI and production imported meshes are not established by these checks.
