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
use two rows with a horizontal divider. Session 55 refines the owner's requested hierarchy:
three compact Phosphor icons select Brush/Eraser/Fill directly; Size and Opacity are unboxed
scrubbable numbers (click to type, Shift-drag for precision). Size uses the accent color.
Flow has a thin amount indicator and opens a slider popover; Hardness uses a small dot indicator
and its own slider popover. Sliders close on outside click / Escape, retaining their identity
while values change. Fill disables brush-specific settings in both the bar and tiny menu.
Anti-aliasing keeps its full name. Tiny docks expose settings behind the sliders icon and
viewport preferences in View. The default dock gives more width to the canvas (80% left of
the right sidebar, then 85% right of the left sidebar); saved custom layouts remain intact.
Nothing extends outside the dock's bounds in the tested widths.

This is tool state already covered by existing brush presets. No new saved setting, schema
migration, shortcut, or paint-kernel change is introduced. The bar is shown for Brush, Eraser
and Polygon Fill; the main row disables brush-specific controls for Fill, while opacity remains relevant.
Future 2D overlays/top-bar expansion should dispatch the same app commands rather than copy
brush state into widgets or add another preset field.

Session 59 moves existing StrokeOptions into one mirror-icon popover in the 3D bar: independent
X/Y/Z mirror toggles plus Lazy Mouse 0–100px. Its active color indicates any mirror or lazy
distance. The sidebar Stroke section and rail X-symmetry toggle are removed; duplicated tool
selection also moves out of the sidebar, leaving Paint target there. SetStrokeOptions validates
and clamps lazy distance in app, retains the active-stroke guard, and changes no document
history. Existing presets and the X mirror shortcut use the same options. No new schema or
preset field is introduced. The popup stays open while adjusting settings.
