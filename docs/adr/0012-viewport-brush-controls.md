# ADR-0012 — Viewport brush controls share app tool state

Status: Accepted (2026-10-18).

The owner moved the essential brush controls ahead of the remaining Phase 1 work. Size, Flow,
Opacity and Hardness belong beside the existing 3D View controls, with a subtle separator.
The side Brush panel retains materials, presets and advanced settings; its duplicate essentials
are replaced by a location hint. This is a toolbar slice, not the complete Phase 2 overlay system.

`ui::brush_bar` renders the widgets; `ui::viewport_bar` handles placement with the viewport
preferences. Widgets copy `Session::brush.settings`, edit that local presentation value and
dispatch `Command::SetBrushSettings` only on change. App validates/clamps the essentials.
There is no second settings store, project mutation, or document undo entry. Existing shortcuts
and presets read/write the same app settings. Active strokes retain their captured settings;
the command follows the existing StrokeInProgress guard.

Wide viewports put both groups in one row, separated by a vertical hairline. Narrower viewports
use two rows with a horizontal divider. Size/Hardness keep short sliders where space permits;
Flow/Opacity are draggable percentage boxes. All four fields allow double-click text entry.
Smaller docks use numeric controls; tiny docks expose settings in the tool menu and viewport
preferences in View. Nothing extends outside the dock's bounds in the tested widths.

This is tool state already covered by existing brush presets. No new saved setting, schema
migration, shortcut, or paint-kernel change is introduced. The bar is shown for Brush, Eraser
and Polygon Fill; the main row disables brush-specific controls for Fill, while opacity remains relevant.
Future 2D overlays/top-bar expansion should dispatch the same app commands rather than copy
brush state into widgets or add another preset field.
