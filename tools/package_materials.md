# Packaging the SubCraft Materials pack

`tools/package_materials.py` turns a materials source folder (such as `D:\SubCraft-SmartMaterials`)
into the release assets the app's **Assets > Materials > Download SubCraft Materials** button
fetches. The source folder is only read.

## Requirements

Python 3.9+ with Pillow (`pip install pillow`). EXR maps, if any, are resized only when `imageio`
and `numpy` are installed; otherwise they are copied unresized (a warning says so).

## Usage

```
python tools/package_materials.py D:/SubCraft-SmartMaterials --version 1 --out dist
```

| Option | Default | |
|---|---|---|
| `--version N` | required | pack version: the release tag is `materials-vN` |
| `--out DIR` | `dist` | output folder (must not be inside the source) |
| `--sizes` | `1k,2k` | which zips to build |
| `--only a,b` | all | only these material folders (for a quick test) |
| `--jobs N` | `2` | worker processes |

It writes `subcraft-materials-vN-1k.zip` and `subcraft-materials-vN-2k.zip`, each with a
`.zip.sha256` file next to it (`<hex>  <file name>`, as `sha256sum` writes). The same input gives
the same bytes (fixed timestamps, sorted entries).

## What goes in

- Every sub-folder with a `material.json`, except folders starting with `_` or `.` (tools, caches).
- Each map listed under `maps` in `material.json`, resized to 1024² or 2048² (Lanczos). PNGs stay
  PNG, and 16-bit height maps stay 16-bit. A listed map that is missing is left out with a warning.
  A material with no map files at all is skipped.
- `material.json`, with `resolution` set to the zip's size. Its `smart_material` and `source`
  entries are removed, because those files are not shipped.
- `preview.png`, and `INDEX.md` at the root.

The zip's top level is the pack itself, which is what the app unpacks:

```
INDEX.md
wood_ebony/material.json
wood_ebony/preview.png
wood_ebony/wood_ebony_basecolor.png
...
```

**Upscaling:** sources made at 1024² are upscaled for the 2K zip, and the script warns about each
map. Generate the sources at 2048² to get a real 2K pack.

## Publishing

1. Run the script, then `python tools/test_package_materials.py` for a quick self-check.
2. Create the GitHub release `materials-vN` in Aalzard/subcraft. It is separate from the app's
   `v…` releases, so don't mark it as the latest app release.
3. Upload the two zips and their two `.sha256` files.

The app uses the newest published `materials-v*` release. GitHub's own asset digest is used when
present, otherwise the `.sha256` file.
