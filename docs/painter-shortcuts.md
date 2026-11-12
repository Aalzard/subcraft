# Substance Painter shortcuts, menus and viewport in SubCraft

Source of the list: Adobe's Substance 3D Painter "Shortcuts" page (Experience League copy,
`experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/shortcuts`, Windows column, editable
and non-editable lists; read again on 2026-11-11, 89 entries). Painter lists `Save as` without a Windows key.

## Key sets

Edit → Settings → Shortcuts (and the "Key set" box in Help → Keyboard shortcuts) switches between:

* **SubCraft (default)**: the original keys plus every Painter key that was free.
* **Substance Painter**: Painter's key wherever the two disagree. Every action has a key in this set (a test checks it).

| Key | SubCraft key set | Substance Painter key set |
|---|---|---|
| `P` | Pen paths | Material picker (Eyedropper); Pen paths on `Shift+P` |
| `B` | Brush | Display next mesh map; Brush on `1` |
| `X` | Toggle X symmetry | Invert the mask brush (white ↔ black); symmetry on `L` |
| `F2` | Rename the selected layer | 3D view only; rename with a double-click |

Everything else is the same in both sets. SubCraft keys Painter does not use (`E`, `Home`, `Ctrl+B`, `Ctrl+Shift+Z`,
`Ctrl+Shift+S`, `Ctrl+Shift+N/F/T`, `Ctrl+A`, `Ctrl+Q`) stay in both.

Menus and tooltips show the binding of the active key set. Help → Keyboard shortcuts (also the button under
Preferences → Keyboard shortcuts) lists, per action, the active key, Painter's key and the SubCraft default, then the
mouse gestures, then Painter's whole list with its status here.

## Your own keys (Settings → Shortcuts)

As in Painter, every action can be rebound:

* the keyboard button next to an action waits for the next combination and makes it the action's only key (Esc cancels);
  `+` adds a key instead; `×` on a key removes it;
* right-click the action (or the reset arrow) puts the key set's default back; "Reset all" forgets every change;
* a key you give to one action leaves the action that had it (the message says which);
* actions without a default key (Eyedropper in the SubCraft set, 3D only, next mesh map, mask invert) can get one;
* the six camera gestures of Painter navigation (rotate, snap rotate, translate, zoom, rotate environment, rotate
  the key light) are rebound with the cursor button, then by clicking the capture pad with the new button and Ctrl / Shift / Alt held;
* the gestures that stay fixed (brush size / flow drags, straight lines, stencil keys, clicks on mask thumbnails)
  are listed under "Not editable".

Your changes sit on top of the key set: switching between SubCraft and Painter keeps them. They are saved with the
preferences by action name, so a renamed or removed action never breaks the file.

## Keys added for Painter parity (both sets unless noted)

| Key | Action |
|---|---|
| `Tab` | Hide UI: only the menu bar and the view(s) stay; `Tab` again brings the panels back |
| `F9` | Painting mode: closes the Bake window and leaves Rendering mode |
| `Ctrl+F4` | Close project (back to the sample model, after the unsaved-changes prompt) |
| `F1` / `F3` / `F4` | 3D and 2D side by side / 2D only / swap the views |
| `F2` (Painter set) | 3D only |
| `B` (Painter set), `Shift+B` | Next / previous baked mesh map on the model, through the material view |
| `X` (Painter set) | Mask brush value g → 1 − g |
| `Alt+Q` | Isolate the selected texture set (hides the others' geometry); again shows all |
| `C`, `Shift+C`, `M` | Unchanged, but they now also leave the mesh map viewer |

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

Bold = added in the Painter-parity pass. The right-click Properties popup at the cursor is PR #131.

## Menus

The menu bar has Painter's top menus, in Painter's order:

| Menu | Items |
|---|---|
| **File** | New… · Open… · Recent files ▸ · Open sample · Close (`Ctrl+F4`) · Save · Save as… · Import resources ▸ (texture set, smart materials, smart material folder) · Export textures… · Export 2D view… · Render image / turntable… · Send to ▸ (PhotoCraft) · Quit |
| **Edit** | Undo · Redo · Layers ▸ (new paint / fill / folder / text, duplicate, delete, select all, copy / cut / paste) · Bake mesh maps… · Project configuration (reload mesh)… · Auto UV unwrap… · Settings… |
| **Mode** | Painting (`F9`) · Bake mesh maps (`F8`) · Rendering (`F10`), the current one marked |
| **Window** | Views ▸ (brings a panel to the front) · Toolbars ▸ (Tools, Status bar ticks) · Brush settings · Hide UI (`Tab`) · Reset UI |
| **Viewport** | Display material (`M`) · Display next / previous channel (`C` / `Shift+C`) · Display next / previous mesh map (`B` / `Shift+B`) · Show entire mesh (`F`) · 3D \| 2D (`F1`) · 3D only (`F2`) · 2D only (`F3`) · Swap (`F4`) · Perspective (`F5`) · Orthographic (`F6`) · Isolate the texture set (`Alt+Q`) · Render quality… · Viewport hints · Height bump |
| **Plugins** | empty: SubCraft has no scripting API yet |
| **Help** | unchanged |

The old Layer and View menus are gone: their items are in Edit ▸ Layers, File ▸ Import resources, File ▸ Send to,
Window and Viewport. Preferences is Edit ▸ Settings, as in Painter.

Not in the menus yet (Painter has them): Save and compact, Save as copy, Save as template, Clean, Export mesh,
the quick mask entries, Views ticks that close a panel, the Log, Display settings and Shader settings views.

## Not available

26 of Painter's 89 entries have no SubCraft feature behind them and stay unbound (their keys stay free in the Painter
key set so they never do something else): the fill manipulator (`Q W E R T + -`, `Shift+W`, `Shift+V`), particles
(`Ctrl+1/2/3`), the projection tool (`3`), absolute-source clone (`Ctrl+6`), quick mask (`U Y I`), geometry mask
(`Alt+H`), engine pause (`Shift+Esc`), resume stroke (`A`), animation (`Space`), content-only copy / paste
(`Ctrl+Shift+C/V`) and Ctrl+drag of a material onto a part.

## Full table

See [painter-shortcuts-table.md](painter-shortcuts-table.md). It is generated from `PAINTER_REFERENCE` in
`crates/app/src/actions.rs`; a unit test fails when the file and the code differ (rerun it with
`SUBCRAFT_WRITE_DOCS=1` to rewrite the file). Other tests keep every bound row consistent with both key sets, check
that no key is bound twice in a set and that every action has a key in the Painter set.
