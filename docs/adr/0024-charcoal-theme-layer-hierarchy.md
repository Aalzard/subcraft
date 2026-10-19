# ADR-0024 — Neutral charcoal theme and layer hierarchy

Status: Accepted (2026-10-19)

## Decision

Use neutral charcoal surfaces, warm gray text and a restrained amber accent across the
existing UI. Preserve near-square buttons (2-point corners). Slider rails are three points
high, with circular handles. Primary and secondary text retain at least 4.5:1 contrast on
panel, alternate-row and selected backgrounds; decorative/tertiary labels remain quieter.
The default interaction height is 18 points to keep slider handles compact; button padding
and explicit icon sizes retain larger controls. The viewport toolbar keeps 26 points so its
Resolution labels/combos align with fixed-size tool icons at every responsive breakpoint.

Layer rows reserve 48 points for independent 36-point content/mask thumbnails, a name and a
secondary type/effect-count line. Blend mode and opacity share a quiet right-hand column.
Nested effect rows use 26 points, smaller type and connecting lines. Parts/History retain
their existing 34-point list rows and the same alternating palette/selection contract.

Only the selected thumbnail's effect domain is expanded, as before. Its header can collapse
the rows without changing selection, effect state or the document. Expansion is temporary
egui state keyed by layer UUID and content/mask domain. All document interactions still
dispatch existing app commands. This is presentation only: no schema, compositor, undo,
thumbnail-evaluation or material-rendering changes.

## Consequences

The reference informs visual hierarchy and color, rather than replacing SubCraft workflows.
Larger previews cost no additional evaluation: the existing bounded asynchronous 64-pixel
preview cache is reused. Long names truncate within their allocated column; hovering keeps
the existing domain/tooltips available. Future effect types can reuse the compact row helper.

## Preview sizing follow-up (session 80)

Shelf and Layers each reserve a fixed footer below their vertical scroller for a small
Preview size slider. UI preferences persist independent sizes (88/36 points by default;
48–160 for Shelf, 24–64 for Layers), using serde defaults for older preference files. This
does not change the project schema or source resolution. Layers cap the effective preview
size to the available width and adapt parent row height; effect rows remain compact.

The shelf header switches to a category dropdown in narrow docks. Hints live behind an info
icon. Header/search controls must not expand the content width: the asset area wraps within
the panel's explicit width and scrolls vertically. Names are limited to one line so small
previews cannot overlap the next row. Existing preview sources/caches and asset commands
remain authoritative; resizing the UI never recomputes document textures.
Shelf and Layers disable egui_dock's outer scrollbars: only their bounded inner lists scroll,
so a previous content width cannot expand the grid and the footer cannot scroll away.
