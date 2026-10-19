# ADR-0026 — Curvature direction at exact boundaries

Status: Accepted (2026-10-19)

The curvature baker normally measures normal change toward the closest point on a neighboring
triangle, weighted by distance. At coincident points it instead measures toward that triangle's
centroid. The centroid can point substantially along the crease; normal change has no component
in that direction, so the normalized dot product weakens. Exact vertices and clamped UV-gutter
samples consequently differ from approaching surface samples, depending on tessellation. Edges
masks can turn that drop into a hole at high thresholds.

For the existing near-zero-distance fallback only, remove the centroid direction's component
parallel to `n.cross(nq)` before normalization. Keep its across-crease sign, proximity weight,
normal interpolation and all ordinary samples. Nearly parallel/opposed normals retain the
fallback without projection. Flat normals still contribute nothing; inward corners remain
concave. The extra vector operations are limited to coincident samples; there is no additional
surface traversal, image pass or UV dilation.

This corrects newly baked Curvature maps only. Preserve stored maps, undo history and schema 7;
users explicitly rebake to update old projects. Do not silently rewrite authored assets on load.
No generator default or breakup setting changes. This does not implement surface-distance
spread or lower-layer normal evaluation; those require the stage contract in ADR-0016.

Acceptance: exact and approaching corners on all six faces/eight vertices, subdivisions 1/4/13,
baked atlas rim texels, concavity/flatness at multiple scales, and high-threshold generator output
through save/reopen. Native evidence: screenshots/edge-wear-connected-corners.png. The owner's
original mesh and production high-poly cases remain unverified.
