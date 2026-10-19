# Material workflow requests — 2026-10-19

Owner's requested scope. `[x]` implemented and tested; `[~]` partial (remaining work stated);
`[ ]` pending. See progress.md for exact native evidence and hand-check limits.

## Workspace presentation (new reference request)

- [x] Shelf-left default; Parts / Texture Sets / History bottom-center tabs. Wide Texture Sets
  uses horizontal selection, resolution/filter and bake/mesh sections (ADR-0025).

- [x] Shelf wraps downward within panel width. Shelf and Layers have independent preview-size
  sliders beneath their scrollable lists; settings persist without altering document textures.

- [x] Neutral charcoal surfaces, calmer text/amber accent, thin round-handle sliders.
- [x] Larger independent layer/mask previews, name/type/effect-count hierarchy and quieter
  blend/opacity column; compact indented effect rows with connecting lines and stack folding.
  Existing document commands, thumbnail cache and content/mask editing behavior are retained.

## Generators

- [~] Correct baked curvature at exact hard corners and clamped UV-border vertices so
  high-threshold Edges masks stay connected. Numeric tests cover every cube corner/face,
  three subdivisions, concavity/scale and save/reopen. Native screenshot in session 82.
  Existing stored curvature must be rebaked for that correction. This isolated-cube fix alone
  did not alone solve the default crate's contact-curvature gaps. Session 84 additionally
  prioritizes intrinsic connected-surface curvature over other components' contact curvature;
  rebaking fixes those gaps even with Natural disabled. Flat contact curvature is retained.
  Owner rejected the session 84 result as still unsatisfactory: overall Edge Wear remains
  unresolved. Claude handoff: PROMPT-claude-edge-wear.md. Prior test results are historical
  evidence, not owner acceptance or proof that the remaining visual problem is fixed.
- [x] Pattern Size, Reduce Repetition, Breakup starting at 0.3 and Random Seed/Next.
  Existing saved recipes retain legacy noise (schema 7 migration; ADR-0022).
- [~] Natural Edge Wear adds Spread/Variation; Spread controls reach independently of Level without revealing
  distant flat faces. Connected convex crease sources avoid contact-curvature cancellation on
  the default crate. Exact on convex planar patches, conservative vertex paths elsewhere;
  UV-seam matching includes float-rounding tolerance. Numeric/undo/migration/worker checks and
  native actual-crate captures pass (session 83); Breakup 0 retains an irregular boundary.
  Session 84 removes the Level multiplier, so 0.065/12.5% has full selected reach.
  Full spread from baked-only/painted-normal detail and exact continuous curved-surface
  propagation remain pending; this does not complete Dirt's source/stage contract.
- [~] Existing Cavity generator (displayed as Dirt) works on black/other masks from baked AO,
  with the new size/breakup/seed controls. Surface spread distance, near-edge intensity and
  normal-detail-aware accumulation remain pending; this is not the complete requested Dirt tool.
- [ ] Both generators automatically read painted/other normal detail strictly below their layer,
  including reorder/undo changes, without anchors. Disable Micro Effects bypasses those details.
- [ ] Numeric acceptance across UV seams/tile boundaries, thin/curved meshes, nested groups;
  dirty dependency/halo handling and complete synchronous export/save convergence.

## Local effects (owner's next milestone, session 85)

- [x] Content Contrast/HSL/Threshold/Clamp and mask Contrast/Threshold/Clamp with dedicated
  Properties, existing nested rows, blend/opacity/order/visibility/undo and evaluated previews.
- [x] Folder local Filter/Levels adjustments: isolated children or passthrough lower stack,
  limited by the folder mask/opacity. Project/Smart Material/Mask schema 9; older stacks migrate.
- [x] Previous-stage CPU neighborhood DAG and bounded tile cache; conservative chained dirty
  halos, UV-space Gaussian Blur and unsharp Sharpen on content/masks/folder adjustments.
  Blur filters premultiplied coverage; Sharpen keeps alpha. Thumbnails scale radius (ADR-0030).
- [x] Directional Blur and Highpass extend the same content/mask/folder stages (ADR-0031);
  angle/radius/amount controls, signed neutral Height and exact legacy schema-10 migration.
- [x] Bevel and Mask Outline on content/masks/folder stages: threshold, radius, relief amount
  and Inner/Outer/Both borders (ADR-0032). Height supports recessed signed relief.
- [ ] Render-owned GPU parity, references and full 4K desktop frame profiling (layer roadmap).

## Channels

- [~] Compact channel toggles implemented for fill layers, Fill effects and brush materials;
  independent click, Alt-click solo and hidden rows are tested. Brush toggles choose
  future-stroke channels; per-paint-layer visibility/solo of existing paint remains pending.

## Baking

- [x] Automatic refresh on path/source replacement (250 ms file checks, async imports) and
  return to Material after every successful bake. Stale-result rejection retained.

## Emission

- [ ] Reuse the existing Emissive channel as Emission, exposed through the same channel toggles.
- [ ] Independent emission intensity, including glow/glare disabled; existing export semantics.
- [ ] Subtle nonzero default glow, independent toggle, strength/radius/spread inside Emission.
- [ ] Optional horizontal/vertical/both glare with proportion controls.
- [ ] Customizable starburst/shine intensity and spread; real-time viewport update.
- [ ] Numeric GPU tests, HDR/tonemap contract and save/migration; preview effects do not bake
  screen-space glare into exported PBR textures.

## Viewport and projection

- [x] Albedo Bilinear/Nearest only in Texture Sets, removed from every 3D toolbar/menu variant;
  document filter still drives CPU composites/exports and GPU preview.
- [x] Exact red symmetry-plane guide in 3D, immediate X/Y/Z updates, hidden when disabled;
  Show guides switch inside the symmetry popup, matching app painting coordinates.
- [x] Rotation immediately below Tiling in the shared texture Projection editor; independent of
  tiling/scale and updated live across supported projections/channels/layers.

Keep the UI consistent and quiet; preserve old projects. These requests take priority over
the older layer/brush roadmaps until the owner steers otherwise.
