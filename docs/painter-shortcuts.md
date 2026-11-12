# Substance Painter shortcuts in SubCraft

Source of the list: Adobe's Substance 3D Painter "Shortcuts" page (Experience League copy,
`experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/shortcuts`, Windows column, editable
and non-editable lists). The helpx.adobe.com copy returned HTTP 403, so the Experience League copy is the only source
read; nothing was added from memory. Painter lists `Save as` without a Windows key.

## Key sets

Preferences → Keyboard shortcuts (and the "Key set" box in Help → Keyboard shortcuts) switches between:

* **SubCraft (default)**: the original keys plus every Painter key that was free (`1 2 4 5 6`, `L`, `D`, `C`, `Shift+C`,
  `M`, `F`, `F5`, `F6`, `F8`, `Ctrl+N`, `Ctrl+Shift+B`, `Alt+F4`, ...).
* **Substance Painter**: the same, plus the two keys where Painter and SubCraft disagree:
  * `P`: Pen paths in SubCraft, "pick stroke material" in Painter -> `P` = Eyedropper tool, Pen paths moves to `Shift+P`.
  * `X`: symmetry in SubCraft, "invert grayscale tool" in Painter (no equivalent here) -> `X` is unbound, symmetry is on
    `L` (Painter's symmetry key, also bound in the default set).

SubCraft bindings Painter does not use (`B`, `E`, `Home`, `Ctrl+B`, `Ctrl+Shift+Z`, `Ctrl+Shift+S`, ...) stay in both
sets. There were no user-defined shortcuts before this change (the table was fixed), so there was nothing to
migrate; the choice is one preference (`keymap`, default SubCraft; missing in old files = SubCraft).

Menus and tooltips show the binding of the active key set. The cheat sheet window has search and lists, per action,
the active key, Painter's key and the SubCraft default, then the mouse gestures, then Painter's whole list with status.

## Mouse gestures

| Gesture | Effect |
|---|---|
| Alt+LMB / RMB drag | Orbit, **around the point of the model under the cursor** (issue #115; Preferences → "Orbit around the point under the cursor", on by default). Over the background the last point is kept; `F` forgets it |
| **Alt+Shift+LMB drag** | Orbit snapped every 90° |
| MMB, Alt+MMB or Ctrl+Alt+LMB drag | Pan |
| Alt+RMB drag / wheel | Zoom |
| Shift+RMB drag | Rotate the environment |
| Ctrl+RMB drag | Brush size (left-right) and hardness (up-down) |
| Ctrl+LMB drag | Brush flow (left-right) and angle (up-down) |
| Shift+click | Straight line from the last stroke point |
| **Ctrl+Shift+click** | Straight line snapped to 5° steps |
| **Ctrl+MMB click** | Depth of field: focus the point under the cursor (turns depth of field on) |
| **Ctrl+Alt+RMB click** | Select the texture set under the cursor |
| S + LMB / MMB / RMB drag | Stencil rotate / move / scale; **Shift+S + LMB** snaps the rotation to 90° |
| N (hold) | Ignore the stencil |
| V + click (Clone) | Set the clone source (Ctrl+click still works) |
| Alt+click / Shift+click a mask thumbnail | Show the mask / disable-enable it |
| Blender navigation: Alt+MMB click | Center the view on the point under the cursor |

Bold = added in the navigation-parity pass. The right-click Properties popup at the cursor is PR #131.

## Full table

See [painter-shortcuts-table.md](painter-shortcuts-table.md) (generated from `PAINTER_REFERENCE` in
`crates/app/src/actions.rs`; a unit test fails when the file and the code differ, rerun it with
`SUBCRAFT_WRITE_DOCS=1` to rewrite the file; other tests keep every bound row consistent with both key sets).
