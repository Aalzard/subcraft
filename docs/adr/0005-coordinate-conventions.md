# ADR-0005 — Coordinate conventions

Status: Accepted (2026-10-13)

## Decision

* **Object space** is the painting space: right-handed, +Y up, counter-clockwise front faces.
  There is no model transform in the engine; the camera's view matrix maps object → view.
* **UV**: origin at the top-left of the texture, v grows downward (glTF). Texel `(x, y)` has its
  center at `((x + 0.5)/w, (y + 0.5)/h)`. Triangles are wrapped into the 0..1 tile containing
  their centroid (the integer offset is the UDIM tile, reserved for later).
* **Tiles**: 64² texels, index `ty · tiles_x + tx`, row-major; texel `i` of a tile is
  `(x0 + i % 64, y0 + i / 64)`.
* **Tangent space**: MikkTSpace computed in OpenGL UV space → tangent-space normal maps use the
  OpenGL (+Y) convention; stored encoded `n · 0.5 + 0.5`, flat = (0.5, 0.5, 1).
* **Normalized position** (fills, generators): `(p − bounds.min) / max_extent` of the whole mesh,
  so every texture set shares one frame and the frame is isotropic.
* **Planar frames** keep images upright viewed down the axis with +Y up and v downward:
  X → (z, 1 − y), Y → (x, z), Z → (x, 1 − y). Triplanar weights `|n_i|^sharpness`, normalized.
* **Camera**: wgpu conventions — view looks down −Z, NDC depth 0..1. Screen space is pixels with
  the origin top-left and y down. View depth = −z in view space.
* **Brush**: size is the diameter in percent of the mesh bounding-box diagonal (object space).
  The tangent frame at a hit has `tangent` = camera right projected onto the surface and
  `bitangent = tangent × normal` (screen-down), matching screen axes for Camera alignment; the
  brush angle rotates the footprint within that (tangent, bitangent) frame.
* **Symmetry** planes pass through the mesh bounds center; a mirrored stamp is produced by the
  camera reflected across the plane (`view · R`), which also mirrors visibility and culling.
