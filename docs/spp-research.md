# Substance Painter .spp research (session 102, 2026-10-22)

Sample inspected read-only: a 65 MB project of the owner's own (`chalice_.spp`, made with a recent
Painter 10.x). Nothing from it is stored in this repository.

## Container
- An `.spp` is an **HDF5 file** (magic `89 48 44 46 0D 0A 1A 0A`). HDF5 is an open format; any HDF5
  reader works (Python: `h5py`; Rust: the `hdf5` crate needs the C library, so a pure-Rust reader or a
  small hand-written one for this file's simple layout is preferable). Datasets are plain uint8 byte
  blobs, no HDF5 compression.

## Layout (groups / datasets)
| Path | What it is (from header bytes) |
|---|---|
| `paint/scene.assbin` | **Assimp binary dump** of the mesh (`ASSIMP.binary-dump...`), format documented in the Assimp sources. This is how the model can be recovered. |
| `paint/document.bin` | Qt `QDataStream` (big endian, UTF-16 strings) starting `DataDocument ...`: the layer stack / texture sets / channels. Proprietary schema, to be decoded by reading many samples. |
| `paint/default_material.bin` | Default material settings. |
| `resources/<sha1>[.image/.sbsar/.shader]` + `resources/.alg_meta/...` | Resources used by the project: `.sbsar` (Substance archives: procedural materials/filters), `.image` (imported images), extension-less large blobs (megabytes; likely painted layer / mesh-map bitmaps), a shader. `resources.toc` maps names like `/World Space Normals Chalice?version=...` and `/Curvature Map from Mesh Chalice?...` to these blobs. |
| `texture-cache/<id>/<id>/<n>` + `texture-cache/header` | Cached texture data (Qt-serialized, tiled; keys look like per-layer/channel buffers, `bufferSize` properties). Likely where painted pixels live. |
| `baking/baking.ini`, `editor/*.ini/json`, `projectsettings.ini` | Plain INI/JSON settings: bake settings, camera, viewer, export preset (`exporterv2.json`), UV tile/atlas settings. Readable as text. |

## What an importer could realistically deliver (in order)
1. **Mesh**: parse `scene.assbin` (documented) into a SubCraft mesh + material slots. Low risk.
2. **Baked maps**: decode the mesh-map resources named in `resources.toc` (normals, curvature, AO...) once their
   bitmap encoding is understood (needs a few more samples).
3. **Texture sets / channels / layer names and blend modes** from `document.bin`: reverse-engineering the Qt stream;
   medium risk, version-dependent.
4. **Painted pixels** from the texture cache / large blobs, flattened per channel first.
5. **Procedural layers (sbsar)**: SubCraft has no Substance engine; these can only be baked to images, not edited.

## Not yet done
- No code written. No sample with a known ground truth (e.g. a project with one fill layer and one stroke, plus its
  exported textures) yet: that is the best next fixture to decode `document.bin`.
- The ArtStation sample (DBAL-A2) was not obtained: its download needs a signed-in account (owner action).
