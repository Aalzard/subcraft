# Substance Painter .spp research

Findings from reading eleven Painter 10.x projects of the owner's own (nothing from them is stored in this
repository; the tests use a synthetic project). Code: `crates/mesh/src/spp/` (reader),
`crates/app/src/spp_import.rs` (session import), `crates/mesh/examples/spp_probe.rs` (developer tool:
`cargo run --release -p subcraft-mesh --example spp_probe -- file.spp [out_dir|-] [max_size]` prints what is
recovered and optionally dumps images as PPM/PGM).

## Container
An `.spp` is an **HDF5 file** (superblock v0, symbol-table groups, v1 object headers, uncompressed contiguous
or chunked byte datasets). `spp/hdf5.rs` is a small hand-written read-only reader (no C library, no new
dependency). Everything below is a byte dataset in it:

| Path | Content | Read by |
|---|---|---|
| `paint/scene.assbin` | Assimp-style binary dump of the model | `assbin.rs` |
| `paint/document.bin` | texture sets, channels, layer stack | `document.rs` |
| `resources.toc` | list of image resources: name + `version=<sha1>` | `images.rs` |
| `resources/<sha1>` | baked mesh maps (and imported images): property header + JPEG 2000 tiles | `images.rs`, `jp2.rs` |
| `resources/<sha1>.sbsar`, `.image`, `.shader`, `.alg_meta/*` | procedural archives, imported images, metadata | not read |
| `texture-cache/<texture set uid>/<n>/<channel flag>` | **flattened channel textures** as of the last save, LZ4 | `images.rs`, `lz4.rs` |
| `baking/baking.ini`, `editor/*`, `projectsettings.ini`, `plugins/*` | settings (INI/JSON/Qt blobs) | not read |

`<texture set uid>` is the decimal `sceneMaterialUID` of the texture set in the document; `<n>` was
3864292196 in every project; `<channel flag>` is the channel's type flag (below).

## `paint/document.bin` (fully decodable)
Little endian (not a QDataStream, despite the `Data...` class names): magic `0x1B7C2FDD`, version 1, three
counts, then one root object. An object is `id: u32`, a class word, then its fields in schema order. The
class word is `0xFFFFFFFF` plus the class definition (name, property count, `(name, type)` pairs) the first
time a class occurs, and the class index afterwards. Property types, established by decoding all eleven
files to **exactly their last byte**:

| Type | Encoding |
|---|---|
| 1 | f32 (also 2/3/4 = 2/3/4 f32 vectors) |
| 5, 9 | i32 (9 = enums) |
| 6 | two i32 |
| 10 | bool, one byte |
| 11 | u32 |
| 12, 21 | u64 (uid; 21 = channel type flags) |
| 13, 14 | 3x3 / 4x4 f32 matrix |
| 15 | two u32 |
| 16 | string: u32 length + UTF-8 |
| 17 | list of embedded structs: count; if > 0 one class word; then the structs without ids |
| 18, 20 | object: id (0 = null), class word, fields |
| 19 | list of objects (each as 18) |

Classes seen: `DataDocument` (materials, `normalMapSyscoord`), `DataMaterial` (= a **texture set**:
`sceneMaterialName`, `sceneMaterialUID`, `resolutionLog2` (two i32), `additionalMaps` = baked map resource
URLs with a usage id, `stacks`), `DataMaterialStack` (`channels`, `stack`), `DataChannel` (`type` flag,
`format`), `DataStackLayers`, `DataLayerColor` / `DataLayerGroup` (label, enabled, mask actions, per channel
blending, group `subStack`), `DataStackActions` / `DataActionGroup`, `DataActionFill` (sources:
`DataSourceUniform` color, `DataSourceBitmap`, `DataSourceProcedural` with an `.sbsar` URL and tweaks),
`DataActionPaint` (strokes with brush parameters and sample points), `DataActionFilterProcedural`,
`DataBlending` (mode id, channels, opacity), `DataGridDeformation`, `DataBrushStamp`... Layers are stored
bottom to top. Brush strokes and deformation grids are only counted by the reader (they are bulk).

Channel flags (checked against the texture-cache keys): 1 Base Color, 2 Height, 4 Specular Edge Color,
8 Opacity, 0x10 Emissive, 0x40 Glossiness, 0x80 Roughness, 0x2000 Metallic, 0x400000 Normal,
0x800000 Ambient Occlusion. Blending mode ids (25 appears to be Normal, others unmapped) and the meaning of
`normalMapSyscoord` (0 in all eleven projects; assumed to be Painter's default, DirectX normals) are **not
verified**.

## Baked mesh maps (`resources/<sha1>`)
`resources.toc` is `u32 1`, then per resource kind: a QString kind (`Alg::ResourceImage`), a count and
QStrings `/<name>?version=<sha1>`. Names: `World Space Normals X`, `Normal Map from Mesh X`,
`Color Map from Mesh X`, `Ambient Occlusion Map from Mesh X`, `Curvature Map from Mesh X`,
`Thickness Map from Mesh X`, `Position X` (X = texture set name).

The blob starts with a Qt property map (big endian: count, QString key + QVariant): `compression` = `jp2`,
`format` = `LUM16` (1 component: AO, curvature, thickness) or `RGBA16` (normal, world normal, color/ID,
position), `width`, `height` (the bake size, 4096 or 8192, independent of the texture-set size),
`tilesCount`. Then, per tile, a 24-byte little-endian record `x0 y0 x1 y1 length 0` followed by `length`
bytes of a **JP2 file** (512x512), padded to a multiple of 8 bytes. Tiles are in hash order; the rectangle
places them. The codestreams are OpenJPEG output: lossless (reversible 5/3 wavelet, RCT for 3 components),
5 decomposition levels, 64x64 code blocks, no precincts, one layer, LRCP. `jp2.rs` is a from-scratch decoder
for that family (tier 1 and 2, 5/3 inverse wavelet, RCT, reduced-resolution decoding by skipping the finest
levels); it is bit-exact against OpenJPEG on a real tile and on the synthetic fixtures. A 4096x4096 map
decodes in 0.3 to 1.5 s (release), in 20-40 ms when the texture set is 512x512 (reduced decode). Rows are
top first and line up with the importer's V flip (UV coverage vs baked alpha: 1.000 in four projects; the
flipped alternative 0.55 to 0.93). Background texels are 0 with alpha 0 (RGBA maps); LUM16 maps carry no
alpha, the importer takes the coverage from a RGBA map of the same set.

## Flattened textures (`texture-cache/...`)
Per channel flag one blob: Qt property map (`compression` = `lz4`, `width`, `height`, `bufferSize`,
`internalFormat` as an OpenGL enum), then `u32` big-endian compressed length and a **raw LZ4 block**
decompressing to exactly `bufferSize`. Formats seen: 35907 `SRGB8_ALPHA8` (Base Color, Emissive), 33323 `RG8`
(Roughness, Metallic, AO, Opacity: value + alpha), 33327 `RG16F` (Height), 34842 `RGBA16F` (Normal, 0..1
encoded). The size is the texture-set resolution (512 for the chalice, 4096 for the others). This is the
final, evaluated result of the whole stack including procedural layers, so it is what gets imported. It is
what Painter had cached at the last save; a channel that was never evaluated (for example Height in a
project that never touched it) is simply absent.

## Model (`paint/scene.assbin`)
Documented in `assbin.rs`. It carries no material names; the texture set name comes from the document. With
one texture set (all eleven projects) the model is that set's. Which part belongs to which set in a
multi-set project could not be determined (no sample has more than one): the importer puts everything into
the first set and says so.

## What the app imports (`Session::open_mesh` on a `.spp`)
Model, one texture set per material with Painter's channels and resolution (clamped to 4096), the baked maps
(dilated into the gutter, resampled to the set resolution), the flattened channel textures as one paint
layer "Painter project (flattened)" plus an empty "Layer 1" above, the export normal format, and notes
listing the Painter layers by name. Base Color / Emissive are converted from sRGB to linear; the Normal
channel is flipped to SubCraft's OpenGL storage under the DirectX assumption above.

Evidence for the normal orientation: Painter's own material graphs expose a `normal_format` input tagged
`$normal_map_format` (0 = DirectX, 1 = OpenGL) that Painter fills from the project setting. In the one
project that stores it (CarInteriro, 19 tweaks) it is 0, and `normalMapSyscoord` is 0 in all eleven, so
DirectX is now well supported, though still not proven for every file.

## Rebuilt layer stack (session 104)
`Session::open_spp` also rebuilds the Painter stack (`crates/app/src/spp_stack.rs`) as a folder
"Painter stack (reconstructed)" between the flattened layer and an empty "Layer 1":
* folders and layers with names and visibility; uniform fills (exact, per channel, UV or tri-planar);
* masks as effect stacks: uniform fills, Levels (exact), `mg_dirt` / `mg_curvature` / edge wear /
  `mg_ambient_occlusion` / `mg_position` as SubCraft generators (approximate), noise graphs as procedural
  patterns (approximate), `blur` / `sharpen` / `clamp` filters (approximate, `clamp` exact), and grunge graphs
  from the **JPEG embedded in the project's `.sbsar`** with Levels for balance/contrast/invert (approximate);
* kept as named hidden placeholders: layers without a mappable fill (procedural materials, filter layers,
  paint/eraser/decal-only layers); masks whose every effect is unmapped are left off; painted mask strokes,
  decals and anchors are not rebuilt (counted in the notes);
* **quality gate:** the rebuilt stack is composited alone (at no more than 512 texels) and compared with
  Painter's flattened channels over the covered texels. Only if Base Color < 0.04, Roughness < 0.06 and
  Metallic < 0.06 (mean absolute difference, 0..1) is it shown and the flattened layer hidden; otherwise it
  is hidden and the notes list the differences. On the 11 sample projects none passes yet (Base Color
  differences 0.015 to 0.57), mainly because Painter's blend-mode ids are unmapped (everything is Normal) and
  procedural materials and painted strokes are missing.
* Embedded images are read at import time into the user's project only; nothing is bundled.

`.sbsar` files are 7z archives (LZMA, plain or LZMA-compressed header); `spp/sevenz.rs` reads them
(LZMA only, no LZMA2) and `SppFile::embedded_jpegs` returns the JPEGs inside the compiled graph. All 134
archives referenced by the samples unpack.

Open time (release build, 4096 texture sets): about 14 s (baked maps 10 s, flattened channels 2.6 s, stack 1.6 s);
the 8192-baked car interior took 94 s before the shared-gutter and parallel dilation changes.

## Cannot be recovered
* Procedural layers (`.sbsar` fills, generators, filters): there is no Substance engine here; only their
  flattened result exists, in the cache.
* The layer structure with masks, blend modes and opacities as editable SubCraft layers: the data is in the
  document (and parsed into `LayerInfo`), but Painter's blend-mode ids are not mapped and most layers are
  procedural, so recreating the stack would be wrong more often than right. Fill layers with uniform colors
  and bitmap sources would be the first candidates.
* Brush strokes as strokes (the stamps, dynamics and projections are not reproduced); their painted result is
  in the flattened texture.
* Multi-texture-set assignment of model parts (see above), UV tile projects (not seen), anchor points,
  environment/viewer settings, export presets.

## Not verified
* DirectX vs OpenGL meaning of `normalMapSyscoord = 0` (assumed DirectX). A height-gradient correlation test
  was inconclusive.
* Height units and scale (taken as stored).
* Projects with several texture sets, with UV tiles, with 8-bit/other JPEG 2000 variants (irreversible 9/7,
  code-block style flags, user precincts, several tiles per file are rejected with an error, never guessed).
* Anything written by a Painter version other than the 10.x ones sampled.

## Gate calibration (Session 105)

All 11 samples still fail the gate. The dominant errors are missing content, not parameter scaling:
Substance material fill graphs (metal_sandblasted, steel_003_bitmap, concrete_asphalt, metal_brushed), painted
strokes, mask decals and unmapped blend ids. Use `SUBCRAFT_SPP_DEBUG=1` to print rebuilt vs Painter channel
means. Per-sample numbers are in `docs/progress.md` Session 105. Blend ids stay unmapped until a one-layer-per-mode
calibration project is available; parameter scalings (blur, sharpen, HSL, dirt, edge, grunge) need a project
whose layers are all rebuildable to be measurable.

## Matching Painter's own result (Session 110)

Findings from rebuilding the stack of three projects against their flattened channels (tools:
`crates/app/examples/spp_lab.rs`, see its header). All were checked numerically on real files.

### Data the earlier reader dropped
* **Polygon fills** (`DataActionDecal` hits, `DataDecalHitPunctual` / `DataDecalHitRect`): every
  top-level folder of a large project is masked by them. A hit stores the click (`position`, or
  `position0`/`position1` for a rectangle) in screen units (0..1, origin **bottom-left**, normalized
  by the viewport height so x can leave 0..1 on wide viewports), the **inverse view-projection**
  (`projViewInvMatrix`, column major, OpenGL clip space), `granularity` (0 triangle, 2 mesh object,
  3 UV island), `color[0]` (value written; 0 erases), `paintThrough`, `viewType` (0 = made in the 2D
  view: positions are UVs) and symmetry copies (`duplicateCount`, `duplicateMatrix`, a mirror).
  With this reading every click of DBAL-A2's folder masks lands on the mesh; the transposed or
  y-flipped readings miss up to two thirds. "Mesh" granularity selects the model file's object.
* **Strokes** (`DataActionPaint` / `DataActionEraser`): per stroke the screen positions, a view and
  an OpenGL projection matrix (orthographic in the samples), `randomSeed`, `pressure` (empty in the
  samples), `viewType` and symmetry copies. Brush: `brushSize` with `brushSizeSpace` 1 is a fraction
  of the mesh bounding-box diagonal (SubCraft's brush size in percent), `flow`, emitter `spacing`,
  `alignment` (0 wrap, 1 planar, 2 camera, 3 UV, SubCraft's order), action `opacity`, the mask value
  in `sourceColor` (uniform), the tip in `sourceTransparent` (`shape` with `hardness`, or a brush
  graph), a screen **stencil** in `sourceStencil` (bitmap) placed by the action's 3x3 `projMatrix`
  (clip-space screen position to stencil UV). The labels of DBAL-A2 are such stencilled strokes.
  2D-view strokes (`viewType` 0) are in UV; their size matches Painter best at 0.15 x `brushSize`
  of the UV square (one project, Street_cyberpunk; the 2D view's zoom is not stored).
* **Anchors**: `DataActionAnchor` (`uid`) in a mask; `DataSourceReference.reference.source[0]` is
  the anchor uid (a u32 pair: read it exactly, uids exceed f32 precision), with its own Levels.
* **Per-action blending**: every action has `perChannelBlending`. Mask fills carry their own mode
  (ids 4, 29, 10, 11 seen), filters carry id 1. Fills carry `uvTransformation` (tiling up to 32x).
* **Imported images** (`resources/<sha1>.image`): Qt properties, then either JPEG 2000 tiles (as the
  baked maps) or one LZ4 block of `dataSizeLZ4` bytes, rows **top first** (unlike the texture
  cache; stencil text reads upright only so), then `ICCdataSize` bytes of ICC profile. Formats
  BGRA8, LUM8, RGBA16, RGBA16F. They hold bitmap fills, mask bitmaps, stencils, and mesh maps baked
  outside Painter.
* `additionalMaps` usage ids (the same in all three projects): 0 ID, 1 ambient occlusion, 2
  curvature, 3 world-space normals, 4 position, 5 thickness. DBAL-A2's AO and curvature are
  imported `.image` maps; the earlier importer left all mesh-map generators without them.
* Texture cache: normal texels no layer wrote are (0, 0, 0) and are imported as flat. Ambient
  occlusion texels no layer wrote are 0 (47% of CarInteriro); not changed (not in the gate).

### Blending ids (search against the flattened result, DBAL-A2)
Found by coordinate descent over 18 SubCraft modes per id with everything else fixed, refitting the
fitted fills for every trial (score: sum of the mean differences of all channels, DBAL-A2 at 256):

| id | where | mode | evidence |
|---:|---|---|---|
| 25 | default entry of every action group, filter layers | the channel's default (Normal; normal-map combine on Normal) | |
| 4 | mask action groups (polygon fills, strokes) | Multiply | 0.659 -> 0.594; Darken / Linear burn tie on binary masks |
| 29 | mask fills (grunges over text / edge masks) | Subtract | Roughness 0.234 -> 0.095; every other mode worse by >= 0.007 |
| 10 | mask fills (references), Height | Multiply | 0.245 -> 0.204; Linear burn 0.207, Darken 0.208 |
| 11 | layer Roughness / Base Color variation layers | Soft light | 0.202 -> 0.172; Color 0.172 |
| 9 | Base Color | Color | Darken 0.0004 worse |
| 15 | white Base Color layers at 0.02-0.26 opacity | Soft light | vs Normal: Base Color 0.051 -> 0.024; ties with Overlay, Hard light, Color, Divide |
| 2 | Normal (folders), Base Color at low opacity | Hard light on non-Normal channels | Base Color 0.024 -> 0.022 only; the Normal channel keeps normal-map combine (not measured) |
| 1 | filters | (the filter replaces; ignored) | |

Ids that coincide with one mode (4 and 10) may be different Painter modes that act alike on these
masks; a one-layer-per-mode calibration project would settle it.
