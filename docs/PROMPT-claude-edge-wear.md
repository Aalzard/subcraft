# Claude handoff - Edge Wear, brush wrapping and regression audit

Work in E:\Applications\ArtCraft\SubCraft on feature/brush-roadmap. Stay on this branch.
Git metadata: E:\Applications\ArtCraft\SubCraft\.git
Current test application: E:\Applications\ArtCraft\SubCraft\target\debug\subcraft.exe
Do not push or release. Verify which executable you launch; older builds exist.

Read first: CLAUDE.md, docs/progress.md, docs/HANDOFF-codex.md,
docs/material-workflow-roadmap.md, docs/layer-system-roadmap.md, docs/brush-roadmap.md,
and ADRs 0003, 0005, 0008, 0018, 0026-0034. Inspect git status and HEAD before editing.
Feature checkpoint: a995822; preceding documentation checkpoint: d3081ea. This handoff is
newer documentation. Latest verified baseline: 538 workspace tests pass, zero failed/ignored;
Clippy passes with existing warnings. Project schema is 12. Do not rebuild delivered features.

Do a baseline inspection, then finish the following items in order. Fix confirmed regressions,
not merely list them. The highest priority is the unresolved Edge Wear defect.

1. FIX EDGE WEAR PROPERLY

The owner still rejects it despite previous fixes/tests. Reproduce on the DEFAULT CRATE after
baking, with Natural Edge Wear OFF and ON. Problems:
- Wear has gaps/disconnections at corners and edge junctions, including after baking.
- Boundaries look unnaturally straight/uniform even at Breakup 0.
- Level 0.065 with Spread 12.5 (maximum) barely reaches outward. Low Level must still permit
  broad useful spread from the edges, without exposing the entire flat face.
- Level, Spread, Softness, Variation, Breakup, Pattern Size and Seed should be predictable;
  adjusting reach should not require increasing Level globally.

Trace bake curvature, geometry/distance sources, generator evaluation, projection/sampling,
worker caches, previews and the actual running build. Start at:
crates/bake/src/curvature.rs
crates/materials/src/sample/edge_distance.rs
crates/compositor/src/mask.rs (eval_edge_spread)
Review ADRs 0026-0028 and the earlier rejected attempts. Replace an approach if its model is
wrong. Do not hard-code the crate, hide the defect behind Natural mode, blur everything, or
weaken acceptance tests. Test convex/concave corners, UV seams, hard/rounded/thin meshes and
nearby separate/overlapping parts. Preserve legitimate concavity and old projects.

2. FIX BRUSH STRETCHING ACROSS HARD EDGES

Painting the top face may legitimately wrap onto the adjoining side. That overlap is wanted.
The defect is that the side receives a stretched/smeared dab instead of a clean coherent mark.
Do not solve it by preventing paint from reaching the adjacent face.

Reproduce on a cube/default crate with a circular/checker/striped alpha and a normal-texture
brush. Investigate stamp footprint, surface/tangent frame, UV Jacobians, per-face sampling,
interpolation and normal orientation. Start at crates/paint/src/stamp.rs, kernel.rs,
stroke.rs and settings.rs, plus app brush-texture routing. Distinguish intentional camera
projection from incorrect surface wrapping. Correct the intended wrap behavior without
breaking UV/camera/tangent modes. Verify consistent world-space brush scale, no side-face
elongation, cracks or detached marks, appropriate occlusion/backfaces, and continuous textured
normal orientation. Cover ordinary paint, masks and Paint effects, undo/redo and export.
Add numeric regressions with controlled 90-degree corners, rounded edges, different UV scales,
seams and irregular triangles. Show before/after evidence in the real app.

3. IMMEDIATE BRUSH SELECTION + VIEWPORT FEEDBACK + DOUBLE-CLICK EDIT

In Shelf > Brushes, single-clicking a brush/preset must immediately make it usable. Ensure
this works from the other tools and preserves the selected layer/mask/Paint-effect destination.
Current shelf handlers are in crates/ui/src/panels/shelf.rs; preset application is in
crates/app/src/brush_preset.rs. Alpha selection currently also opens an asset preview; review
that interaction so single-click is a direct painting action.

Show a small, calm notification INSIDE the 3D viewport's TOP-LEFT corner, for example
"Brush: Scratches" with a small tip thumbnail if practical. A new selection updates it once;
it remains briefly, then fades out. Anchor it to the viewport, avoid toolbar overlap and
pointer interception, and keep the existing restrained theme. It must acknowledge the brush
that actually became active. Reuse existing feedback/state infrastructure where appropriate.

Double-clicking a brush opens its Brush settings/editor for that brush so the owner can edit
it. Reuse the existing panel/preset workflow. Handle double-click without conflicting actions
or duplicate notifications. Keep selection as tool state, preserve mask targeting and one undo
per actual stroke. UI draws/dispatches; app owns brush/application logic. Test both clicks,
selection while another tool is active, notification replacement/fade and editor focus.

4. DEEPLY AUDIT ALL RECENT FEATURES AND FIX REAL BUGS

Recent delivered work:
- Contrast/HSL/Threshold/Clamp and folder Filter/Levels adjustments.
- Gaussian Blur, Sharpen, Directional Blur and Highpass.
- Bevel and Mask Outline (Inner/Outer/Both).
- Inclusive per-effect thumbnails, target-channel encoding, shared size control and correct
  passthrough/isolated folder preview inputs.
- Effect drag reorder: row/name/thumbnail, insertion line, edge scroll, Escape, one undo,
  selected-effect/brush routing and typed payload isolation.
Bevel Smooth/Height Adjust, general GPU filter parity, references/lower-normal generator
inputs and emission enhancements are still pending. Do not report them as implemented.

Start with doc/src/filter.rs; materials/src/filter.rs and filter/contour.rs;
compositor/src/stages.rs, cache.rs and tile.rs; app/src/layer_previews.rs and effect_stack.rs;
app/src/session.rs; ui/src/panels/filter.rs, effects.rs, layers.rs and ui/src/layer_previews.rs.
Read ADRs 0029-0034 before changing semantics.

Check numeric correctness, alpha/premultiplied coverage, linear color versus raw masks/data,
signed Height, neutral/zero settings, supported channel domains, spatial tile halos, image
boundaries, scaled previews, folders, disabled effects, order/blend/opacity, stale worker jobs,
undo/redo, save/reopen, sync export and migration. Investigate atlas-island mixing explicitly;
it is a documented UV-filter limit, not evidence that geometric seam-aware filtering exists.
Audit invalid/no-op/stale drops, cancellation, payload preservation and selection stability.
Native post-drop screenshots exist, but physical native drag gestures were not hand-verified;
most interaction checks used headless egui events. Likewise CPU profiles exclude GPU frames.

QUALITY / DELIVERY

Follow CLAUDE.md dependency rules: logic in paint/app/materials/compositor; no egui/wgpu in
logic crates; UI only dispatches/draws. Use immutable COW tiled storage, deterministic kernels,
versioned schema migrations if document data changes, and complete preset persistence.
Preserve unrelated Cargo.lock, crates/ui/Cargo.toml, crates/ui/examples/, artworks/Dead-Man/,
untracked zips, output/, generate_textures.py and __pycache__/. Do not bypass the previous
cleanup rejection or kill the owner's running app. Coordinate shared files before concurrent
editing. Do not drive the physical mouse without the owner's permission; native scripted
captures and headless pointer-event tests are available.

Add unit plus independent numeric/golden acceptance tests for texel behavior. Run before EACH
small working commit, in PowerShell:
$env:CARGO_INCREMENTAL='0'
$env:CARGO_PROFILE_DEV_DEBUG='line-tables-only'
cargo test --workspace -j 1
cargo clippy --workspace --all-targets -j 1

Use existing examples/performance tools; profile 4K long strokes, repeated edits, worker
convergence and actual GPU/render/upload frame time separately. Do not equate short CPU polls
with smooth desktop FPS. Confirm source changes are in the tested executable. Save native
screenshots in docs/screenshots/ and state precisely what was not verified by hand. Tests alone
do not establish that the owner's Edge Wear problem is fixed.

At session end update progress, roadmaps, getting-started, significant ADRs and HANDOFF-codex
with commits, pass/fail counts, evidence, remaining defects and next concrete steps. Commit docs
last. Provide the exact application path for owner testing. Finish complete sub-items rather
than partially touching everything. Thanks, and have a nice day.
