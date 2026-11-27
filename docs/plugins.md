# Plugins

A SubCraft plugin is a folder you can share. It can add:

* **Scripts** that show up in the **Plugins** menu and run on the open project.
* **Assets** that show up in the Assets panel: smart materials, smart masks, brush presets,
  brush alphas, textures and environments, plus export presets (Export Textures window) and
  bake presets (Bake window).

Plugins hold no compiled code. Scripts are [Rhai](https://rhai.rs/book/) and run through the
same tool API as the [scripting guide](scripting.md), and assets are the same files SubCraft
saves itself. So one plugin works on Windows and Linux alike, and a broken plugin cannot crash
the app.

## Installing and managing plugins

Open **Plugins > Manage plugins...**:

* **Install from .zip...** installs a plugin someone sent you. Installing a newer version of a
  plugin you already have replaces the old one (they are matched by the plugin's `id`).
* **Install example plugins** adds *Weathering Tools* (scripts) and *Painted Metal Pack*
  (smart materials, smart masks, export presets and a script) to look at and copy.
* The checkbox switches a plugin on or off. A plugin that is off keeps its files but adds
  nothing. Plugins are on when installed.
* **Remove** deletes the plugin's folder (click twice).

Plugins live in `Documents/SubCraft/Plugins`, or in the folder named by the `SUBCRAFT_PLUGINS`
environment variable. **Plugins > Open plugins folder** opens it. Copying a plugin folder in by
hand works too: use **Reload** in the Plugins window afterwards.

Scripts can change and save your project and export files, like any script you run. Only install
plugins from people you trust.

## Writing a plugin

The quickest start: type a name under **New plugin** in the Plugins window and click
**Create**. SubCraft makes the folder with a `plugin.json`, a `scripts/hello.rhai` and empty
asset folders, and opens it.

A plugin folder looks like this:

```text
My Plugin/
  plugin.json
  scripts/
    Add_rust.rhai           -> Plugins > My Plugin > Add rust
  library/
    smart-materials/        .scsmart files
    smart-masks/            .scmask files
    brush-presets/          .scbrush files
    alphas/                 images (PNG, TGA, JPEG, EXR)
    textures/               images
    environments/           HDR / EXR / PNG / JPEG
    export-presets/         export preset .json files
    bake-presets/           bake preset .json files
    substances/             Substance archives (.sbsar): materials and generators
  README.md                 optional, for people browsing the folder
```

Everything except `plugin.json` is optional. `library/` has the same layout as your own asset
library (`Documents/SubCraft/Library`), and sub-folders inside each asset folder become
categories in the Assets panel.

### plugin.json

```json
{
  "format": "subcraft-plugin",
  "schema": 1,
  "id": "3f2c9a3e-8a51-4c0e-9d0a-2b7f4b9d6c11",
  "name": "My Plugin",
  "version": "1.0.0",
  "author": "Your name",
  "description": "One or two sentences about what it adds.",
  "homepage": "https://example.com/my-plugin",
  "min_subcraft_version": "0.7.0"
}
```

| Field | Required | Meaning |
| --- | --- | --- |
| `format` | yes | Always `"subcraft-plugin"`. |
| `schema` | yes | Always `1` for now. A newer SubCraft may read newer schemas. |
| `id` | yes | A [UUID](https://www.uuidgenerator.net/) you make once and never change. It tells two plugins apart and links a new version to the old one. Create gives you one. |
| `name` | yes | Shown in the Plugins menu and window, and as the library name in the Assets panel. |
| `version` | no | Your plugin's version. |
| `author` | no | Shown next to the name. |
| `description` | no | Shown in the Plugins window. |
| `homepage` | no | An `https://` link shown in the Plugins window. |
| `min_subcraft_version` | no | The oldest SubCraft the plugin works with. Older versions list the plugin with the reason and load nothing from it. |

### Scripts

Each `.rhai` file directly in `scripts/` becomes one menu entry. The file name is the label:
underscores and dashes read as spaces (`Add_rust.rhai` shows as "Add rust"). A run is one undo
step, `print` output goes to the log and its last line to the status bar. See the
[scripting guide](scripting.md) for the API and the tool list.

### Assets

Make the assets in SubCraft and copy the files from your library into the plugin's `library/`:

* Smart materials and smart masks: right-click a layer > *Create smart material* / *Create smart
  mask*. The files land in `Documents/SubCraft/Library/smart-materials` and `smart-masks`.
* Brush presets: Brush panel > save a preset (`brush-presets`).
* Export presets: Export Textures window > Output templates (`export-presets`).
* Bake presets: Bake window > save a preset (`bake-presets`).
* Images (alphas, textures, environments) are plain image files.
* Substance materials and generators are `.sbsar` files (`substances`).

Export and bake presets from a plugin are read-only in SubCraft: duplicate one to change it.

### Sharing

Zip the plugin folder (the zip may hold the folder itself, or its contents with `plugin.json` at
the top) and share the zip. To ship an update, raise `version`, keep the same `id`, and zip it
again: installing it replaces the old version.

## Examples

The two example plugins are in [`docs/plugins/examples`](plugins/examples):

* [`weathering-tools`](plugins/examples/weathering-tools): three scripts (edge wear, ground
  grime, a layer stack report).
* [`painted-metal-pack`](plugins/examples/painted-metal-pack): two smart materials, two smart
  masks, Godot 4 and Roblox export presets, and one script.
