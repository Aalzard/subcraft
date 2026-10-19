# Material workflow requests — 2026-10-19

Owner's requested scope. `[x]` implemented and tested; `[~]` partial (remaining work stated);
`[ ]` pending. See progress.md for exact native evidence and hand-check limits.

## Workspace presentation (new reference request)

- [x] Shelf wraps downward within panel width. Shelf and Layers have independent preview-size
  sliders beneath their scrollable lists; settings persist without altering document textures.

- [x] Neutral charcoal surfaces, calmer text/amber accent, thin round-handle sliders.
- [x] Larger independent layer/mask previews, name/type/effect-count hierarchy and quieter
  blend/opacity column; compact indented effect rows with connecting lines and stack folding.
  Existing document commands, thumbnail cache and content/mask editing behavior are retained.

## Generators

- [x] Pattern Size, Reduce Repetition, Breakup starting at 0.3 and Random Seed/Next.
  Existing saved recipes retain legacy noise (schema 7 migration; ADR-0022).
- [ ] Increasing edge Level spreads wear outward from edges without revealing whole flat faces;
  predictable behavior across meshes, with a real surface-distance contract.
- [~] Existing Cavity generator (displayed as Dirt) works on black/other masks from baked AO,
  with the new size/breakup/seed controls. Surface spread distance, near-edge intensity and
  normal-detail-aware accumulation remain pending; this is not the complete requested Dirt tool.
- [ ] Both generators automatically read painted/other normal detail strictly below their layer,
  including reorder/undo changes, without anchors. Disable Micro Effects bypasses those details.
- [ ] Numeric acceptance across UV seams/tile boundaries, thin/curved meshes, nested groups;
  dirty dependency/halo handling and complete synchronous export/save convergence.

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
