# ADR-0043 — Radial symmetry

Status: Accepted (2026-10-30)

## Context

Mirror symmetry paints each stamp again through a camera mirrored across the X, Y or Z plane
through the mesh bounds center (`Camera::mirrored`). The renderer only needs that list of
cameras: each stamp keeps the index of the camera it was cast through, and every camera gets its
own depth map for occlusion. Radial symmetry (a stroke repeated N times around an axis) can use
the same mechanism.

## Decision

* `StrokeOptions::radial` (`RadialSymmetry { enabled, axis: 0..3, count, center_offset }`) holds
  the setting. The axis is the line along X, Y or Z through the bounds center plus
  `center_offset`, in object units. Copy `k` (from 1 to N-1) is the view camera rotated by
  `k * 360 / N` degrees around that line (`Camera::rotated`). Rays from the same screen
  positions therefore land on the rotated surface points. Like mirroring, this assumes the mesh
  has the matching symmetry.
* **Radial and mirror combine.** The camera list is: the view, then its N-1 rotations, then the
  existing mirror expansion applied to all of them. With one mirror plane that gives 2N copies,
  and in general `N * 2^m` for m enabled planes. A mirror plane that contains the axis gives the
  dihedral (kaleidoscope) pattern. A plane perpendicular to the axis doubles the pattern onto
  the other side.
* **At most 64 copies per stroke** (`MAX_SYMMETRY_COPIES`). Each copy needs one depth map the
  size of the viewport (about 8 MB at 1080p), so the radial count is clamped to `64 >> m`
  (`RadialSymmetry::effective_count`). The popover says when the cap applies.
* With radial off, or N = 1, the camera list is exactly the old one, so output is bit-exact.
* `DepthCache::get_all` renders missing maps in parallel and grows to hold every camera of the
  stroke, so later dabs reuse all of them. The next request with fewer cameras shrinks the cache
  back to its default 8 entries.
* The setting lives in brush presets through `StrokeOptions` and uses `#[serde(default)]`. Presets
  written before this change load with radial off. The preset format version stays 2, because
  the change only adds fields and older builds ignore the unknown key.
* `SetStrokeOptions` clamps the count to 2..=64 and the axis to 0..=2, and rejects a center
  offset that is not finite.
* Viewport guide: the radial axis line plus one spoke per copy in the plane through the center.
  They are drawn with the existing red symmetry-guide pass, next to the mirror-plane sections.

## Consequences

A stroke costs one stamp-resolution ray cast and one kernel pass per copy, as mirroring already
did. A 64-copy stroke on a 1080p viewport renders up to 64 depth maps on its first dab, in
parallel, and keeps about 0.5 GB of depth maps until the cache shrinks. Topology-aware symmetry,
a gizmo for the axis, and UV tiling remain separate roadmap items.
