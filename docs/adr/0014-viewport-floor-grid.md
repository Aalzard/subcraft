# ADR-0014 — Depth-tested viewport floor and axis guides

Status: Accepted (2026-10-18).

The grid is a display guide in render, controlled by saved viewport preferences in ui. It does
not enter paint/app, document rasters, exports, brush presets, picking or undo. Old preferences
load a visible grid with red X / blue Z axes and the upward green Y axis disabled. The toolbar
grid icon toggles the whole guide; its right-click popover controls each axis.

The finite XZ floor is centered beneath the whole model, just below its minimum Y, using
1/2/5 world-unit spacing derived from model size. Its axes indicate world directions through
this translated display origin; ADR-0005's model coordinates and symmetry planes are unchanged.
Whole-model bounds keep the guide stable when parts are hidden or isolated.

render::grid caches 246 line vertices per bounds change. Each visible frame updates a 112-byte
uniform and draws one line-list pass after the model, using its depth buffer without writing
depth. Subtle neutral minor/major lines, restrained axis colors, distance fade and grazing-angle
fade avoid a harsh infinite lattice. Both 4x MSAA and single-sample pipelines are provided.
ViewParams::grid is opt-in, so previews/other render callers retain guide-free output.

Acceptance covers bounded geometry/directions, red/blue defaults, optional green Y, all axes
off, exact restoration when hidden, and unchanged opaque model pixels under perspective/ortho
with AA on/off. MSAA silhouette samples may correctly reveal guides behind their uncovered
portion; depth assertions erode the foreground mask by one pixel. Native screenshot evidence
and the limits of hand verification are recorded in progress.md.
