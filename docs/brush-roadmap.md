# Brush roadmap

Owner's ordered roadmap, tracked from 2026-10-17. `[x]` complete, `[~]` partial (remaining work
stated), `[ ]` not started. Resume Phase 1 after the owner's essential-bar priority override
(session 54); the remaining Phase 2 work still follows it. Existing features are extended,
not replaced. See progress.md for exact test and real-app verification evidence.

## Phase 1 — Projection painting

- [x] Per-channel image sources (Base Color, Roughness, Metallic, Height, Normal, and other
  channels present in the texture set), retaining flat values when cleared.
- [x] Paint whole material from named imported maps; unmatched channels disabled. Channels absent
  from the current texture set are reported/skipped, not implicitly added by a tool setting.
- [~] Projection modes: UV, planar, triplanar complete; camera and per-stamp still required.
- [x] Scale, rotation, offset, tiling controls, using fill-layer projection sampling.
- [x] Reuse gloss inversion and DirectX → OpenGL normal conversion.
- [x] Replace / detail-combine normals on paint layers; shared RNM, normalized output.
- [x] Brush channel Choose image / Clear, imported shelf texture drag/drop.
- [ ] Starter-pack texture drag/drop onto brush channels (existing starter drops target fills).
- [x] Native normal-on-paint-layer workflow on an imported OBJ, shaded and channel screenshots,
  undo/redo and a second Detail pass; see session 53 for the remaining hand-verification limits.
- [ ] Projection-frame/mirrored-tangent normal reorientation and final 4K frame pacing on complex models.
- [x] Presets embed channel images and all new brush material settings; legacy presets load.

## Phase 2 — Top brush settings bar and guides in 3D / 2D

- [~] Owner priority override (2026-10-18): the essential 3D bar was requested before finishing
  Phase 1. Tool, Size/Flow/Opacity/Hardness now sit beside viewport controls; tip thumbnail,
  spacing/smoothing, symmetry/projection, texture chips and preset dropdown remain.
- [x] Essential bar uses the same app settings/presets/shortcuts; narrow docks wrap/collapse.
  Duplicate primary controls moved out of the side panel; advanced controls remain there.
- [~] Brush outline (existing 3D cursor); surface-following outline and 2D outline still required.
- [ ] Tip preview and live size/flow/opacity readout during hold-key drags.
- [ ] Lazy-mouse string, straight-line / angle-snap guides with degrees, shape handles.
- [ ] Symmetry planes / radial axis, stencil frame, clone source marker, active texture guides.
- [ ] Active-tool hint line, per-tool don't-show-again, View toggles for every overlay/hint.
- [ ] Mode-toggle toasts and warnings for masked layers, locked channels, fill selection,
  outside Parts selection.
- [ ] Overlay/hint design ADR; render in ui/render from app state only.

## Phase 3 — Smooth and straight strokes

- [~] Smoothing: existing lazy mouse; pulled string, catch-up, post smoothing, speed-adaptive,
  finish-to-cursor and Off/Light/Medium/Heavy/Calligraphy presets remain.
- [~] Existing Shift+click lines; Shift-drag lock, chained polylines, 15/45/90° snapping,
  screen/UV axis locks remain.
- [ ] Projected shapes: line with editable endpoints before commit, rectangle, ellipse,
  polygon, arc, Bézier; outline / filled; stroke pen path with current brush.
- [ ] Repeat last stroke with offset / rotation / mirror; fade-in / fade-out taper.
- [ ] Smoothing model ADR.

## Phase 4 — Tip, dynamics, jitter

- [~] Hardness/roundness/angle exist; falloff curve editor remains.
- [ ] Dual brush; random folder tip; animated sequences; grain/paper in screen/surface space.
- [x] Tip follows stroke direction (existing).
- [~] Size/angle/scatter/flow jitter and seed exist; roundness/opacity/HSV jitter,
  per-stamp vs per-stroke control, spray count and seed lock UI remain.
- [ ] Pressure/tilt/rotation mapping with curves to size, flow, opacity, hardness, angle, height;
  speed dynamics and simulated mouse pressure.

## Phase 5 — Symmetry and repetition

- [x] Multi-axis X/Y/Z mirror through bounds center (existing combinations).
- [ ] Radial symmetry 2–64 around any axis; custom plane/point gizmo.
- [ ] Topological symmetry; UV tiling; array stamping; kaleidoscope.

## Phase 6 — Surface-aware painting

- [ ] Follow hard edges/creases, UV seams/island borders, curvature lines.
- [ ] Shortest surface path between clicks; stitch/rivet stamps along paths.
- [ ] Height/world-axis limits (top only); soft fade at backface-angle limit.
- [ ] One-click edge-wear stroke.

## Phase 7 — Texture-editing brushes

- [ ] Clone/source point; smudge; blur; sharpen; dodge/burn; heal/seam blend.
- [~] Existing context-menu color/material eyedropper; all-channel brush workflow remains.
- [ ] Channel locks; alpha lock; lasso limit; fill by color range / curvature.

## Phase 8 — Smart and automatic brushes

- [ ] Multi-channel + generator smart brushes (rust/wear/dirt).
- [ ] Particle drips/dust/scratches; gravity drips.
- [ ] Decal placement/move/rotation/scale gizmo before commit.
- [ ] Auto-fill closed loops; saved/tiled stencil library.

## Phase 9 — Shortcuts, presets, history

- [ ] Hold-key drag size/flow/opacity/hardness with readout; number-row 1–0 opacity.
- [ ] Radial favorites; recent-brush cycle; X swap colors/materials; Alt eyedropper;
  temporary erase/mask; Space orbit; command palette; shortcut editor; hold ? cheat sheet.
- [~] Existing preset save/load/library/search; folders/tags/favorites, sample-stroke thumbnails,
  Photoshop .abr / Krita tip import remain. New material settings/images are stored.
- [ ] Editable past strokes (size/color/path), stroke record/replay, finished-stroke opacity.
- [ ] Editable-strokes design ADR (pen-source ADR-0008 remains authoritative for existing paths).

## Requirements applying to every item

- [x] Keep paint/app independent of egui/wgpu; UI dispatches new material commands to app.
- [x] One document undo entry per stroke; immutable sources captured at stroke begin.
- [x] Old preset compatibility; versioned project data only when document source data changes.
- [~] Unit/numeric acceptance, workspace tests/clippy, real-app evidence per completed slice;
  future slices require their own checks and screenshots.
- [~] Long-stroke 4K profiling and conservative tile bounds (session 52) with reproducible `brush_projection` example; sustained
  end-to-end frame pacing on production meshes is still an acceptance gate.
