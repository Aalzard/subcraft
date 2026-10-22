# ADR-0025 — Shelf-left workspace and responsive Texture Sets

Status: Accepted (2026-10-19)

The owner requested Shelf on the left, with Parts, Texture Sets and History sharing a tabbed
node below the center viewport. Layers/Properties remain on the right. This is now the normal
layout, rather than only a screenshot fixture. Dock persistence moves from v4 to v5 once so
existing installs receive the requested arrangement; later rearrangements remain persistent.
View > Reset layout restores it. Document schemas and UI scaling preferences do not change.

At widths of at least 660 UI points, Texture Sets uses three horizontal regions: the set
list, resolution/albedo controls, and bake/mesh status. Full-panel vertical scrolling is
disabled in the dock. Only a long list of texture sets can scroll within its own column.
The two usual sample sets and all their controls fit a short bottom panel without scrolling.
Narrow sidebars retain the vertical presentation and its own vertical scroll area.

Both orientations share command-dispatch helpers, so selection, undoable resolution/filter
changes and opening Bake behave identically. Compact mode moves the resolution explanation
to a tooltip and uses shorter bake/mesh labels. All document operations stay in app commands.
