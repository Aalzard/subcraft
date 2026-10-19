# Brush roadmap

Owner's ordered roadmap, tracked from 2026-10-17. `[x]` complete, `[~]` partial (remaining work
stated), `[ ]` not started. The owner's UI refinement requests (sessions 54–61) are complete.
The owner's newer layer/mask/effect foundation request takes priority; follow
**layer-system-roadmap.md** first, then resume brush Phase 1 before the remaining Phase 2 work. Existing features are extended,
not replaced. See progress.md for exact test and real-app verification evidence.

Performance prerequisite (session 70): desktop viewport compositing now runs on immutable
snapshots in a worker; sparse texture initialization uploads allocated tiles only. This does
not complete the remaining brush tools, general layer filters or GPU compositor roadmap.

Session 82 fixes baked curvature weakening at exact hard corners and UV rims (rebake Curvature
to update existing assets). This completes a generator defect fix, not an additional brush
phase or surface-aware edge-wear stroke tool. Surface spread and lower-layer normal inputs
remain the next material-workflow milestone.

Session 83 introduces optional natural geometric Edge Wear spread/variation (schema 8), with
legacy generators retained on migration. This is a material mask operation, not the pending
surface-aware brush stroke tool or lower-layer normal dependency work. See ADR-0027 and the
material-workflow checklist for the distance approximation and remaining source types.

Session 84 fixes baked contact cancellation even with Natural disabled, and separates Spread
from Level (including 0.065/12.5%). This corrects those generator defects; no additional brush
phase is complete. Stored Curvature maps need rebaking. See ADR-0028.

Session 85: owner still rejects Edge Wear; see the Claude prompt and material-workflow roadmap.
Local non-destructive filters/folder adjustments advance the layer roadmap (ADR-0029).
Session 86 adds UV-space Blur/Sharpen on content/masks, including folder stages, bounded tile
neighborhood evaluation and scaled previews (ADR-0030). This advances the layer roadmap.
Session 87 extends the same layer stages with Directional Blur/Highpass (ADR-0031).
Session 88 adds contour Bevel and Mask Outline on the same layer stages (ADR-0032).
Session 89 adds per-effect stage thumbnails and correct folder preview inputs (ADR-0033).
Session 90 adds effect drag reorder with one undo and independent domain selection (ADR-0034).
Owner follow-up (session 91; Claude handoff, implementation pending):
- [ ] Clean surface-wrapped painting across hard corners without stretched side-face dabs.
- [ ] Immediate Shelf brush activation, fading top-left viewport selection notice and
  double-click brush editor, preserving layer/mask/Paint-effect targeting.
See PROMPT-claude-edge-wear.md for priority, source entry points and acceptance checks.
The ordered brush-phase checklist below remains unchanged.

Session 73 acceptance artwork: CORE-07 is a saved 4K cube with real high-to-low bakes, editable
materials/masks/generators and a native screenshot. Its creation/reopen/export checks do not
replace the pending interactive stroke/frame-pacing checks or complete additional brush phases.

Session 74 refines that separate artwork with deeper rebaked relief, procedural roughness,
localized scuffs/dust and steel-only rust. This adds asset acceptance coverage, not brush tools.

Session 75 repairs high-poly baking: UV-less source import, a live normal display layer and
persistent bake feedback. No additional brush phase is marked complete (ADR-0019).

Session 76 adds document-level Bilinear/Nearest albedo filtering for pixel art (ADR-0020),
including textured stroke sampling and exports. This does not complete another brush phase.

Session 77 adds bake geometry inspection (blue high/red exposed low, front/rear cage) and
map-by-map live bake display (ADR-0021). Full cage-error classification, custom cages, matching
by name, Height baking and intra-map tile previews remain pending. No brush phase changes.

The newer owner requests are tracked in **material-workflow-roadmap.md** (generators, channel
controls, bake refresh, emission and viewport refinements). Complete those before resuming the
older phases. Their control changes do not imply completion of unfinished brush phases.

Session 79 follows the owner's visual reference: neutral charcoal palette, thin circular-handle
sliders, larger layer/mask previews and compact collapsible nested effects (ADR-0024).
This is a presentation refinement; no unfinished brush phase is marked complete.

Session 80 fixes shelf wrapping in narrow docks and adds independent, remembered Shelf/Layer
preview-size footers. These are display preferences; no additional brush phase is complete.

Session 81 makes Shelf-left / bottom-center utility tabs the normal layout and adapts Texture
Sets to a horizontal bottom strip. The next owner task is Dead Man reference texturing;
unfinished brush phases remain unchanged.

Build housekeeping (session 62): latest tested binary consolidated into
`target/release/subcraft.exe`. Removing obsolete versions/caches remains blocked by automatic
approval review; no artifacts were deleted. This does not change feature completion below.

Session 63 fixes inspector focus: layer selection reveals the matching Brush/Properties tab;
fill edits target the selected fill, with numeric composite/undo and real dock input coverage.

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
  Phase 1. Tool, Size/Flow/Opacity/Hardness and symmetry/Lazy Mouse now sit beside viewport
  controls; tip thumbnail, spacing/advanced smoothing, projection, texture chips and presets remain.
- [x] Essential bar uses the same app settings/presets/shortcuts; narrow docks wrap/collapse.
  Duplicate primary controls moved out of the side panel; advanced controls remain there.
- [x] Toolbar design refinement: compact Brush/Eraser/Fill icons, unboxed Size/Opacity scrubbing,
  Flow meter / Hardness dot with slider popovers, full Anti-aliasing label, wider default canvas.
- [x] Panel readability refinement: subtle alternating Parts/History/Layers rows, taller rows,
  banded Brush/Properties sections and property rows, section/part icons; ADR-0013.
  Session 61 strengthens the initially too-faint charcoal contrast and keeps hover brighter
  than either shade; selection retains priority.
- [x] Owner-requested 3D grid: subtle floor lines, red X / green Y / blue Z guides, Y hidden by
  default, top-toolbar grid icon toggle / right-click axis popup and saved viewport preferences.
  GPU depth/projection/AA acceptance and native CLI screenshot; ADR-0014.
- [x] Calmer material actions/text, 2px almost-square controls, channel cards and header/body
  overlap fix (session 57 plus the owner's radius refinement in session 58).
- [x] Mirror icon in the 3D toolbar opens X/Y/Z toggles and Lazy Mouse slider; sidebar Stroke
  section / duplicate rail symmetry and sidebar tool selector removed; shared app command/presets.
- [x] Owner-requested 95% UI default and UI-size setting first in File / Edit → Preferences,
  75–150%, live updates / Reset, saved between runs. Numeric paint-scale acceptance; ADR-0015.
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
