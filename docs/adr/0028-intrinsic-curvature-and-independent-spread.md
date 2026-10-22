# ADR-0028 — Intrinsic corner curvature and independent wear reach

Status: Accepted (2026-10-19).

The default crate's separate posts contribute concave contact curvature beside the body's
convex corners. The average could cancel the body's own convex signal. ADR-0027's geometric
source hid this only with Natural enabled. Multiplying Spread by twice Level also reduced
12.5% Spread to 1.625% at Level 0.065, contrary to the owner's requested independent controls.

For baking, accumulate both the complete neighborhood and the sampled connected surface's
neighborhood. Prefer intrinsic curvature when that surface has a normal-changing contribution;
otherwise retain the complete contact neighborhood, preserving concavity on a flat floor beside
a box. Connected-element IDs are built once per bake and reused with the ID map. Imported
object identity also separates coincident objects. High-poly bakes use the projected detail
triangle's identity, never the low-poly triangle index. Projection misses retain the complete
neighborhood fallback. No image dilation or per-texel connectivity construction is introduced.

Every neighbor's direction is measured across the crease, preserving its true 3D proximity
weight. Neighbors farther along the same edge previously diluted the average; near-zero
across-crease float errors could even normalize into a full negative contribution. Use the
projected centroid direction when that component is within radius × 0.001, including neighbors
whose full distance is nonzero. This extends ADR-0026 beyond coincident points and removes
tessellation-dependent breaks along straight edges. Convexity signs and flat contact fallback
remain covered by numeric tests. A high-poly projection may legitimately hit a covering part;
acceptance requires its exposed corners, rather than every hidden edge of the low mesh.

Natural Spread now directly specifies percentage of the mesh's longest extent. Level still
selects positive baked detail, but never scales geometric distance. Variation, Breakup,
Softness, Pattern size, Seed, existing dirty tracking and undo commands are unchanged.
Spread stays within connected surfaces, with ADR-0027's curved-surface approximation limits.

This is an evaluator/baker correction with no new serialized data: schema 8 remains valid,
including its existing migrations for schemas 1–7 and smart recipes. All authored settings
and stored maps are retained; there is no lossy parameter migration. Existing Natural masks
with Level other than 0.5 deliberately change width to the selected Spread. Previously baked
maps change only after a rebake; opening a project does not silently overwrite its maps.

Acceptance covers the actual crate with Natural both off/on, metadata-less meshes, flat
contact concavity, exact corners, numeric 0.065/12.5% reach, distant flat-face exclusion,
undo/reopen and worker convergence. Native captures and any unverified hand interactions
are recorded in session 84 of progress.md.
