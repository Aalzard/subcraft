# Painter parity checklist

The goal: someone who knows Substance 3D Painter never gets lost in SubCraft. Every feature, setting,
shortcut and interaction Painter has should exist in SubCraft, in the same place, under the same name.

This folder is the master checklist. Each file covers one area of Painter, row by row, in the order
Painter shows the items, so a file also works as a layout spec for that panel or menu.

Checked against `origin/main` at `297dcb3` (2026-11-11, after 0.3.4). Statuses come from reading the code,
not from running the application.

## Scope

- **Match:** features, setting names, defaults and ranges, panel and tab organisation, menus, context
  menus, the keyboard and mouse map, viewport behaviour, workflows.
- **Never copy:** Adobe's icons, artwork, bundled materials and textures, logos or help text. SubCraft
  draws its own icons and writes its own tooltips.

## Totals

| Area | Covers | Rows | Done | Partial | Missing | n/a | Parity | In-flight PRs cited |
|---|---|---|---|---|---|---|---|---|
| [menus](menus.md) | Main menu, New Project, Project Configuration | 142 | 23 | 48 | 67 | 4 | 34% | #124, #137 |
| [shortcuts](shortcuts.md) | Keyboard and mouse map, shortcut editor | 174 | 52 | 25 | 97 | 0 | 37% | #131, #141 |
| [viewport](viewport.md) | 3D view: navigation, right-click, overlays, gizmos, drops | 229 | 33 | 46 | 150 | 0 | 24% | #131 |
| [toolbar](toolbar.md) | Tools, Docks, Plugins and Contextual toolbars | 163 | 14 | 34 | 114 | 1 | 19% | #131 |
| [layers](layers.md) | Layer stack, blending modes, context menu | 251 | 88 | 56 | 106 | 1 | 46% | - |
| [properties-paint](properties-paint.md) | Properties: paint tools, paths, presets, colour picker | 294 | 30 | 59 | 204 | 1 | 20% | #131, #141 |
| [properties-fill](properties-fill.md) | Properties: fill layers and projections | 191 | 17 | 31 | 143 | 0 | 17% | #131, #141 |
| [masks-effects](masks-effects.md) | Masks and the effect stack | 199 | 84 | 51 | 64 | 0 | 55% | - |
| [generators](generators.md) | Every generator and its parameters | 554 | 24 | 52 | 477 | 1 | 9% | #131 |
| [filters](filters.md) | Every filter and its parameters | 721 | 34 | 54 | 632 | 1 | 8% | - |
| [texture-set-settings](texture-set-settings.md) | Texture Set List and Texture Set Settings | 157 | 25 | 40 | 92 | 0 | 29% | #137 |
| [baking](baking.md) | Baking mode, common and per-map settings | 199 | 33 | 43 | 123 | 0 | 27% | #137 |
| [display-settings](display-settings.md) | Display Settings: environment, camera, post effects, viewport | 192 | 8 | 52 | 132 | 0 | 18% | - |
| [shader-settings](shader-settings.md) | Shader Settings, shaders and their parameters | 136 | 0 | 5 | 131 | 0 | 2% | - |
| [export](export.md) | Export Textures window, presets, mesh export | 212 | 20 | 49 | 142 | 1 | 21% | #124 |
| [shelf-assets](shelf-assets.md) | Assets panel, import, libraries | 255 | 47 | 53 | 141 | 14 | 30% | #124 |
| [preferences](preferences.md) | Settings window, project settings, command line | 163 | 3 | 28 | 111 | 21 | 12% | #137, #141 |
| [history-log](history-log.md) | History panel and Log window | 34 | 11 | 11 | 12 | 0 | 49% | - |
| [view-2d](view-2d.md) | 2D view | 97 | 14 | 16 | 67 | 0 | 23% | #124, #131 |
| **Total** | | **4363** | **560** | **753** | **3005** | **45** | **22%** | |

- **Parity** = (done + half of partial) / (rows - n/a). It counts rows, not effort or how often an item
  is used, so read it as a rough gauge only.
- **Generators and filters** hold 1,275 of the rows, because every parameter of every shipped generator
  and filter has its own row. Without those two files the rest stands at 27% parity.

## How to read a row

`| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |`

- **Status** starts with one of:
  - `done`: the same function is reachable in the same or an equivalent place.
  - `partial`: it exists but differs (fewer options, different range or default, different place or name,
    no shortcut). The cell says what differs.
  - `missing`: nothing in the code.
  - `n/a`: Adobe-service-only (account sign-in, Adobe cloud libraries, Iray licence items).
- **SubCraft location** is `file:line` on `297dcb3` for done and partial rows.
- **PR** names an open pull request that delivers the item. The status is still what `main` has today.
- **(unverified)** in the Default / range column means the item or value is not in Adobe's public
  documentation and was listed from memory. Check it in Painter before building to it.
- **not documented** means Adobe's pages name the setting but give no default or range.

## In-flight work the checklist refers to

| PR | What it delivers | Files that cite it |
|---|---|---|
| #124 | Export preset editor | export.md |
| #131 | Right-click floating Properties at the cursor, curvature modes, polygon fill | viewport.md, toolbar.md, properties-paint.md, generators.md |
| #137 | Baked map import, curvature from normal, ID colours | texture-set-settings.md, baking.md |
| #141 | HSV / RGB / HEX colour picker (no branch on origin yet) | properties-paint.md |

When one of these merges, re-check its rows and change the status.

## Sources

Adobe's public Painter documentation at experienceleague.adobe.com/en/docs/substance-3d-painter, which
replaced helpx.adobe.com/substance-3d-painter (its source is the public repository
`AdobeDocs/substance-3d-painter.en`), including the release notes up to version 12.2. Each file lists
the pages it was built from.

## Known limits of this checklist

- **Defaults and ranges are thin.** Adobe's pages rarely give numbers. Most sliders read "not documented";
  those need a reading from Painter itself before a default is copied.
- **Context menus** (layer, mask, effect, asset, mesh-map slot) appear only as screenshots in Adobe's
  pages. Entries and order are marked "(unverified)" where they were not named in text.
- **Baker settings**: option lists for antialiasing, AO distribution and similar are "(unverified)".
- **Shaders**: Adobe does not document the parameters of the ASM and OpenPBR shaders; only the groups
  named in release notes are listed.
- **Not run.** Nothing here was verified in the running application. A row marked done means the code
  path exists and is wired to the UI.
- All cited `file:line` locations were checked to exist on `297dcb3`.

## Not Painter UI items, so not in the tables

These come from the 2026-11-11 gap audit. They matter to a Painter user, but they are bugs, performance
or platform work rather than checklist rows:

- Mask painting in 3D writes thin lines (issue #95); Wacom clicks not registered (issue #82).
- Pen dynamics: pressure curves, tilt and rotation, pressure on opacity and hardness.
- 4K layered performance; generators re-evaluated with no mask cache; GPU parity for neighbourhood filters.
- The per-frame `grid pipeline` validation error.
- Multiple UV sets, multi-mesh projects, FBX animation.
- Apple Silicon sign-off (issue #85), Wayland tablet input, Windows arm64 start crash, Flatpak / AppImage.
- Translation coverage.

## Keeping it current

- A PR that implements a row changes that row's status and location in the same PR.
- New Painter versions: add rows from the release notes. Never delete a row; change its status.
- The totals table is counted from the files (rows whose Status cell starts with done / partial /
  missing / n/a). Recount when rows change.
