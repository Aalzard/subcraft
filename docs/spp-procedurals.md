# Substance Painter procedural content in .spp projects: inventory and plan

Inventory of everything procedural in the eleven sample projects (the owner's own files, read-only; one,
DBAL-A2, is licensed for private research only and was only parsed). Nothing from them is stored here. Companion
to `docs/spp-research.md` (container, `document.bin` format, what the importer already brings in). Scripts
that produced the numbers: `scripts/spp-inventory/` (Python; needs `h5py` and, for `.sbsar`, `py7zr`; used for
research only, not part of the build).

Counts are over the 11 files together unless stated. "Uses" counts a procedural source or filter once per
layer/mask action (a material fill with 7 channel sources counts 7).

## 1. Layers, effects, filters, generators: what the documents contain

### 1.1 Everything in the stack is one of eight action classes
All 11 files together hold 532 color layers, 72 folders, 11 texture sets. A layer is `DataLayerColor` or
`DataLayerGroup`; what it does is a list of **actions** in `actions` (the layer content) and `maskActions`
(the mask). There is no separate "effect", "filter" or "generator" type: every filter and generator is a
`DataActionFilterProcedural` or a `DataActionFill` whose source is an `.sbsar` graph.

| Action class | In content | In mask | What it is |
|---|---:|---:|---|
| `DataActionFill` | 480 | 445 | fill with uniform color, bitmap, or procedural (`.sbsar`) sources, one per channel; projection UV / tri-planar / mask-specific |
| `DataActionPaint` | 49 | 1448 | brush strokes (stamps + sample points + brush parameters) |
| `DataActionFilterProcedural` | 65 | 167 | a procedural filter (`.sbsar` with an `input`) |
| `DataActionFilterLevels` | 8 | 138 | Levels: per-channel gamma, input/output min/max, clamp (always 0 in the samples) |
| `DataActionEraser` | 17 | 164 | eraser strokes |
| `DataActionDecal` | 0 | 257 | decal hit records (6 files; `DataDecalHitPunctual` 1380, `DataDecalHitRect` 272) |
| `DataActionAnchor` | 6 | 24 | anchor point (a named point other layers can read, via `DataSourceReference`) |
| `DataActionGroup` | | | wrapper that carries the blending of a run of actions and a nested `subStack` |

Fill projections: layer content 0 (UV) 426, 2 (tri-planar) 54; masks 6 160, 2 141, 0 138, 1 6.
The ids' meanings were not verified.

Per file (layers / folders / hidden / with a mask / paint actions / procedural uses / bitmap sources):
CarInteriro 64/13/10/54/119/55/0, CorridorFloor 10/2/1/6/17/6/0, DBAL-A2 337/37/1/294/1091/547/39,
OxygenDeviceProvider 22/5/3/15/12/23/0, RoomEntranceDoor 16/2/2/12/21/7/0, RoomEntranceWall 16/2/2/5/29/5/0,
RoomFloor 13/2/1/6/20/6/0, RoomInteriorWall 12/2/1/7/20/6/0, Street_cyberpunk 14/1/0/14/33/6/0,
TheWalker_ 17/5/0/13/121/14/0, chalice_ 11/1/1/7/14/9/0.

**Smart materials and smart masks are not referenced anywhere.** Painter stores them expanded as ordinary
folders and layers (the `Steel Stained` group in the chalice is one); only the `.sbsar` resources inside keep
their library identity. So "smart material" support means recognising the expanded stack, not resolving a reference.

### 1.2 Procedural graphs referenced
684 procedural uses in layers and masks over **83 distinct graphs** (159 distinct resource URLs, because the
package part of the URL differs per project: `starter_assets`, `starter_assets.<uuid>`, `your_assets.<uuid>`,
and one `tool/shape`). Brush stamps and stencils (`DataActionPaint.sourceColor/Stencil/Transparent`, 1255
`DataBrush` objects, almost all in DBAL-A2) reference further `.sbsar` brush tips (`brush_paint_*`,
`drop_*`) that are not counted below. All 134 referenced `.sbsar` blobs are present in the files.

By role (uses): filters 232, "pattern / grunge / noise" masks and fills 239, mesh-map driven mask generators
(`mg_*`) 160, full PBR materials 53.

Where a graph is used: **procedurals live mostly in masks** (generators, grunges, blur, warp, sharpen).
In layer content they are mostly materials (`Plastic - Old Computer`, `Rubber - Synthetic`, steel, soil,
leather, fabric), `sharpen` and `hsl_perceptive`.

The 48 most used graphs (the other 35 are single uses of grunge/noise/pattern graphs):

| Painter graph | kind | uses | files | where | mesh-map / image inputs | sbsar | key parameters |
|---|---|---:|---:|---|---|---:|---|
| `blur` | filter | 95 | 9 | mask 95 | - | 1 KB | intensity, blur_tilling |
| `mg_metal_edge_wear` | mask | 69 | 7 | mask 69 | ambient_occlusion, curvature, grunge (const), micro_height (const), micro_normal (const), position, world_space_normals | 1942 KB | invert, Wear_Level, Wear_Contrast, Use_Triplanar, Triplanar_Blending_Contrast, Grunge_Amount, grunge_scale |
| `sharpen` | filter | 52 | 1 | content 40, mask 12 | - | 1 KB | intensity |
| `warp` | filter | 48 | 1 | mask 48 | input_noise (const) | 3 KB | warp_mode, intensity, intensity_divider, warpangle, blending_mode, source_type, noise_blur |
| `noise_dirt_3` | pattern | 36 | 1 | mask 34, content 2 | - | 2 KB | invert, balance, contrast, non_square_expansion, scale, disorder |
| `grunge_dirt_thin` | pattern | 25 | 1 | mask 25 | - | 1334 KB | Invert, Balance, Contrast, Flip_Horizontal, Flip_Vertical |
| `mg_dirt` | mask | 24 | 5 | mask 24 | ambient_occlusion, curvature, grunge (const), micro_height (const), micro_normal (const), position, world_space_normals | 23 KB | invert, dirt_level, dirt_contrast, Use_Triplanar, Triplanar_Blending_Contrast, grunge_amount, Grunge_Scale |
| `mg_curvature` | mask | 23 | 3 | mask 23 | curvature, micro_height (const), micro_normal (const), position, texture (const), world_space_normals | 8 KB | global_invert, global_blur, global_balance, global_contrast, curvature_invert, curvature_mode, curvature_sharp |
| `mg_ambient_occlusion` | mask | 21 | 9 | mask 21 | ambient_occlusion, micro_height (const), micro_normal (ref), micro_normal (const), position, texture (const), world_space_normals | 17 KB | global_invert, global_blur, global_balance, global_contrast, ao_invert, ao_blur, ao_balance |
| `fractal_sum_base_colored` | pattern | 21 | 1 | content 12, mask 9 | - | 11 KB | invert, Position, Contrast, non_square_expansion, Roughness, Global_Opacity, Min_Level |
| `Plastic_-_Old_Computer` | material | 18 | 1 | content 13, mask 5 | - | 5403 KB | diffuse, diffuse_intensity, specular, glossiness, glossiness_intensity, base_color, base_color_intensity |
| `hsl_perceptive` | filter | 18 | 1 | content 18 | - | 1 KB | mode, hue, saturation, luminosity |
| `grunge_dusty_scratch` | pattern | 17 | 1 | mask 17 | - | 850 KB | Invert, Balance, Contrast, Flip_Horizontal, Flip_Vertical, tiling_amount, rotate_by_45_degrees |
| `brick_generator` | pattern | 13 | 2 | mask 12, content 1 | - | 5 KB | tile_by_ratio, invert, histogram_position, historgram_contrast, Bricks, Bevel, Keep_Ratio |
| `grunge_dirt_muddy` | pattern | 11 | 1 | mask 11 | - | 1347 KB | Invert, Balance, Contrast, Flip_Horizontal, Flip_Vertical |
| `Rubber - Synthetic` | material | 10 | 1 | content 10 | - | 7106 KB | diffuse, diffuse_intensity, specular, glossiness, glossiness_intensity, base_color, base_color_intensity |
| `blur_slope` | filter | 10 | 1 | mask 10 | input_noise (const) | 5 KB | intensity, intensity_divider, mode_blend, quality, source_type, noise_blur, noise_level |
| `moisture_noise` | pattern | 8 | 2 | content 8 | - | 2 KB | tile_by_ratio, invert, histogram_position, historgram_contrast, Pattern_size_width, Pattern_size_height, Disorder |
| `arrow_long` | pattern | 8 | 1 | mask 8 | - | 2 KB | invert, scale, width, rotation |
| `mg_mask_editor` | mask | 7 | 3 | mask 7 | ambient_occlusion, curvature, micro_height (const), micro_normal (const), position, texture (proc), texture (const), texture_sec (proc), texture_sec (const), thickness, world_space_normals | 22 KB | global_invert, global_blur, global_balan |
| `mg_position` | mask | 7 | 4 | mask 7 | position, texture (const), world_space_normals | 5 KB | global_invert, global_blur, global_balance, global_contrast, position_invert, position_balance, position_contrast |
| `metal_sandblasted` | material | 7 | 6 | content 7 | - | 323 KB | metal_type, metal_color, metal_roughness, grain_intensity, grain_density, random_grain_orientation, channel_diffuse |
| `grunge_leak_dirty` | pattern | 7 | 1 | mask 7 | - | 1174 KB | Invert, Balance, Contrast, Flip_Horizontal, Flip_Vertical |
| `noise_anisotropic_radial` | pattern | 7 | 1 | mask 7 | - | 2 KB | invert, Position, Contrast, angle, angle_random, disorder, pattern_amount |
| `mg_uv_border_distance` | mask | 7 | 1 | mask 7 | - | 1 KB | invert, position, contrast, Intensity, distance |
| `3D_ridged_noise_fractal` | pattern | 6 | 6 | mask 6 | - | 20 KB | invert, Position, Contrast, scale, size, offset, distortion_intensity |
| `grunge_galvanic_large` | pattern | 6 | 1 | mask 6 | - | 5 KB | tile_ratio, balance, contrast, invert |
| `Soil - Mixed` | material | 6 | 1 | content 6 | - | 19575 KB | diffuse, diffuse_intensity, specular, glossiness, glossiness_intensity, base_color, base_color_intensity |
| `grunge_shavings` | pattern | 6 | 1 | mask 6 | - | 8 KB | tile_ratio, balance, contrast, invert |
| `fx_sharpen` | filter | 4 | 3 | content 4 | - | 1 KB | intensity |
| `grunge_concrete_burnt` | pattern | 4 | 1 | mask 4 | - | 3188 KB | balance, contrast, invert, brush_pattern, random_mode, rotation_variation |
| `grunge_leak_spots` | pattern | 4 | 1 | mask 4 | - | 1499 KB | Invert, Balance, Contrast, Flip_Horizontal, Flip_Vertical |
| `grunge_dust_small` | pattern | 4 | 1 | mask 4 | - | 978 KB | Invert, Balance, Contrast, Flip_Horizontal, Flip_Vertical |
| `grunge_wipe_brushed` | pattern | 4 | 1 | mask 4 | - | 1659 KB | Invert, Balance, Contrast, Flip_Horizontal, Flip_Vertical |
| `grunge_pebbles_dirt` | pattern | 3 | 1 | mask 3 | - | 1991 KB | Invert, Balance, Contrast, Flip_Horizontal, Flip_Vertical, tiling_amount, rotate_by_45_degrees |
| `iron_shiny` | material | 3 | 1 | content 3 | - | 3 KB | color, roughness, Scale, grain_intensity, angle |
| `grunge_wipe_leaky` | pattern | 3 | 1 | mask 3 | - | 1318 KB | Invert, Balance, Contrast, Flip_Horizontal, Flip_Vertical |
| `grunge_scratches` | pattern | 3 | 1 | content 2, mask 1 | - | 3356 KB | balance, contrast, invert, brush_pattern, random_mode, rotation_variation |
| `grunge_map_012` | pattern | 3 | 1 | mask 1, content 2 | - | 6 KB | balance, contrast, invert, brush_pattern, tile_ratio |
| `anisotropic_noise` | pattern | 2 | 1 | content 2 | - | 3 KB | tile_by_ratio, invert, histogram_position, historgram_contrast, X_Amount, Y_Amount, Y_By_Resolution |
| `steel_002_bitmap` | material | 2 | 1 | content 2 | - | 814 KB | luminosity, contrast, hue_shift, saturation, normal_intensity, normal_format, roughness_value |
| `noise_fractal_sum_2` | pattern | 2 | 1 | mask 2 | - | 4 KB | invert, balance, contrast, non_square_expansion |
| `noise_fur_1` | pattern | 2 | 1 | content 2 | - | 2 KB | invert, balance, contrast, non_square_expansion, scale, disorder |
| `grunge_dirt_large` | pattern | 2 | 1 | content 2 | - | 1800 KB | Invert, Balance, Contrast, Flip_Horizontal, Flip_Vertical |
| `noise_fractal_sum_1` | pattern | 2 | 1 | mask 2 | - | 4 KB | invert, balance, contrast, non_square_expansion |
| `noise_dirt_5` | pattern | 2 | 1 | mask 2 | - | 2 KB | invert, balance, contrast, non_square_expansion, scale, disorder |
| `noise_clouds_3` | pattern | 2 | 2 | mask 2 | - | 2 KB | invert, balance, contrast, non_square_expansion, scale, disorder |
| `stripes` | pattern | 2 | 1 | mask 2 | - | 2 KB | tile_by_ratio, invert, histogram_position, historgram_contrast, Stripes, Width, Softness |

### 1.3 Parameters and inputs
Every procedural carries a `tweaks` list (`DataTweakFloat`/`Int`/`Float2`/`Int2`/`Float3`/`Float4`, one per exposed
input, keyed by the graph's input identifier, plus `$outputsize` (log2 pair, always 8,8 here, i.e. relative to the
texture set) and `$randomseed`). Values are plain numbers: e.g. `blur.intensity` 0.06 to 0.17,
`sharpen.intensity` 0.08 to 0.15, `mg_dirt.dirt_level` 0.16 to 0.40 and `dirt_contrast` 0 to 0.67,
`mg_metal_edge_wear.Wear_Level` 0.04 to 0.34, grunges `Balance`/`Contrast`/`Invert`/`Flip_Horizontal`/`Flip_Vertical`,
`warp.intensity` 0.001 to 0.006 with `noise_scale` 1 to 14, `brick_generator.Bricks` as an int pair, vector tweaks for colors.
Tweaks are complete: the XML inside each `.sbsar` lists every input with default and slider range, so unset values
are known.

`DataProceduralInputsSource` binds the graph's **image inputs** to sources:
* the `mg_*` generators take `curvature`, `ambient_occlusion`, `world_space_normals`, `position` (and
  `thickness` for `mg_mask_editor`) as `DataSourceBitmap` = the baked mesh maps already imported, plus
  `grunge`, `micro_normal`, `micro_height`, `texture` as `DataSourceUniform` (unused) or, once, a reference;
* filters take their image through `filterinput/input` (the stack below them), `warp` and `blur_slope` also
  `input_noise` (uniform = off);
* `mg_mask_editor` takes two procedural textures (`texture`, `texture_sec`), i.e. generators feeding generators.

Blend modes: layer-level `perChannelBlending` entries exist only where a channel deviates; every
`DataActionGroup` carries a default entry `25 / all channels`. Ids seen on layers: 0, 1, 2, 4, 9, 10, 11, 13, 15,
23, 25, 29 (see section 4). Opacity values are plain floats (0.48, 0.55...).

## 2. What is inside an `.sbsar`

* **It is a 7z archive, not a zip** (`7z BC AF 27 1C`). Entries (all 136 archives): one `assemblies/content/0000/<name>.sbsasm`,
  one `<name>.xml` and, in 9 archives, PNG files (embedded bitmaps). One graph per archive.
* **The XML** (`sbsdescription` 2.1) is the interface only: graph label, keywords, author ("Allegorithmic"),
  GUI groups, `outputs` (uid, identifier, pixel `format` 28 = 8-bit RGBA, 16, 12, 0..., size relative to the output
  size), and `inputs` (uid, identifier, type, default, slider min/max/step, label, for images the channel
  names). Input types seen: 0 float (689), 4 int/bool (547), 8 size (134), 5 image (119), 2/3 vectors, 1, 6.
  Outputs per graph: 1 (120 graphs), the PBR materials 7 to 12 (`basecolor`, `normal`, `roughness`,
  `metallic`, `height`, `ambientocclusion`, `glossiness`, `diffuse`, `specular`...). There is **no node graph** in the XML.
* **The `.sbsasm`** (magic `SBAM`) is the compiled graph. Header: magic, format version (a u32 whose high half is
  2, 3, 4, 5 or 6: 62 archives v2, 36 v4, 23 v5, 12 v3, 3 v6; older Painter content, newer cooker),
  cooker build / uid, total size, section offsets. Size: 196 bytes to 3.5 MB. It contains **no strings**
  (no node or function names), a table of ascending offsets (e.g. 370 entries in `mg_dirt`, strides of 12 to 284 bytes),
  and records that are typed words (`0x09000000 + n`, `0x0a420001`, ...) interleaved with IEEE floats
  (1.0, 0.5, 2.0, 0.0625...). Small graphs such as `blur` (1.4 KB) show a repeating record for a 16-tap kernel
  with explicit weights, which suggests an optimized, partially unrolled dataflow program rather than a
  node list.
* **Embedded bitmaps are plainly readable.** 34 of the referenced archives (32 distinct graphs, 171 uses) contain a standard **JPEG** at a fixed
  position (offset 60 of the `.sbsasm`): every `grunge_*` map (2048x2048 grayscale, 0.8 to 3.5 MB per graph),
  `mg_metal_edge_wear` / `mg_fiber_glass_edge_wear` (their grunge), `steel_002/003_bitmap` (2048x2048 RGB), `iron_rough`,
  `finish_rough` (2 JPEGs), `grunge_map_*_bitmap`, and the brush tips (`brush_paint_*`, `drop_*`, 1024x1024). All 34 decode
  with an ordinary JPEG decoder (checked). For these graphs the whole "procedural" result is: bitmap, then
  balance/contrast/invert/flip tweaks (the grunge graphs have exactly these six parameters), so they can be
  re-created exactly without any engine.
* **How decodable is the compiled graph?** Container, header, offset tables and constants: yes, they are regular.
  The *semantics* (which atomic node kinds exist, how an instruction selects blend, blur, levels, gradient map, noise
  or FX-map, how mesh maps are sampled in the `mg_*` generators) are **not** recoverable from the file alone, because
  the kinds are numeric opcodes with no names and no documentation, spread over five format versions. An interpreter
  would have to be inferred from the 136 samples by experiment, graph by graph, verified against the flattened
  results; the noise nodes (Perlin, Worley, FX-map) would also need to match Adobe's algorithms closely to give the same look.
  I did not attempt it; the numbers above are the honest basis for estimating it (section 5).

## 3. Mapping to SubCraft

SubCraft today: mask generators `Edges` (curvature), `Cavity` (AO, labelled Dirt), `Directional`, `PositionGradient`, `MeshMap(kind)`;
procedural patterns `Clouds`, `Cells`, `Grunge`, `Scratches`, `WoodRings`, `Brushed`, `Checker`, gradients, `Dirt`, `Cracks`,
`Streaks`, `Speckles`, `Pores`; filters `Contrast`, `Hsl`, `Threshold`, `Clamp`, `Blur`, `Sharpen`, `DirectionalBlur`, `Highpass`,
`Bevel`, `MaskOutline`; `Levels`; mask effects Paint / Fill / Generator / Levels / Filter / Invert / Image; 31 blend modes
(including the three normal-map ones); masks per layer with an effect stack; fills with Uniform / Bitmap / Procedural /
MeshMap sources through UV / planar / tri-planar projection; layer paint rasters.

| Painter content | SubCraft counterpart | Fit |
|---|---|---|
| `DataActionFilterLevels` (146) | `Levels` | **1:1** (gamma, in/out min/max per channel; clamp flag) |
| uniform fills (904 sources) | fill with `Uniform` source | **1:1** (Painter stores linear values) |
| `blur` (95), `blur_slope` | `Filter::Blur`; `blur_slope` needs a slope-noise input | blur: approximate (intensity to pixel radius is a calibration); blur_slope: none |
| `sharpen` (52, +`fx_sharpen`) | `Filter::Sharpen` | approximate |
| `hsl_perceptive` (18) | `Filter::Hsl` | approximate (Painter's is perceptual; SubCraft's plain HSL) |
| `clamp`, `bevel`, `finish_brushed_linear` | `Filter::Clamp`, `Filter::Bevel`, `DirectionalBlur` | clamp 1:1; others approximate |
| `warp` (48), `fx_warp` | none | **missing** (needs a noise-driven warp filter) |
| `mg_dirt` (24) | `Generator::Cavity` ("Dirt") | approximate (same inputs: curvature + AO + grunge) |
| `mg_metal_edge_wear` (69), `mg_fiber_glass_edge_wear`, `mg_curvature` (23) | `Generator::Edges` | approximate |
| `mg_ambient_occlusion` (21) | `Generator::MeshMap(AO)` / `Cavity` | approximate |
| `mg_position` (7) | `Generator::PositionGradient` | approximate |
| `mg_mask_editor`, `mg_mask_builder`, `mg_uv_border_distance` | none (multi-input masks; UV border distance) | **missing** |
| `grunge_*` and other bitmap-bearing graphs (32 graphs, 171 uses) | mask `Image` effect + `Levels` for balance/contrast, `Invert`, flips | **exact** if the embedded JPEG is used (licensing, section 6) |
| `noise_dirt_*` (36+), `ratio_dirt_*` | `ProceduralKind::Dirt` / `Grunge` | approximate look, not the same pattern |
| `fractal_sum_*`, `noise_clouds_*`, `3D_ridged_noise_fractal`, `moisture_noise` | `Clouds` | approximate |
| `noise_crystal_*`, `noise_bnw_spots_*`, `noise_fur_*`, `anisotropic_noise*`, `noise_anisotropic_*` | `Cells`, `Speckles`, `Brushed` | approximate |
| `stripes`, `brick_generator` (13), `arrow_long` (8), `shape_*`, `creases_soft` | `Checker` at best | **missing** |
| PBR materials (`steel_00x_bitmap` bitmap-based; `Plastic`, `Rubber`, `Soil`, `metal_*`, `leather`, `fabric` procedural) | fill with Bitmap source per channel (bitmap ones); presets elsewhere | bitmap ones: **exact** (JPEG + tweaks); procedural ones: **missing** |
| `DataActionPaint` / `Eraser` strokes (49 content, 1448 + 164 mask) | the paint engine could replay strokes (positions, pressure, brush settings) | **hard**: Painter's brush engine, alphas and dynamics differ; the result is in the flattened maps only |
| `DataActionDecal` (257), `Anchor` (30) | planar projection fills / none | **missing** |
| Blend modes | SubCraft `BlendMode` | ids unknown (section 4) |
| Mask folders and nested stacks | `Mask` with effect stack; folders | **1:1 structure** |

## 4. Blend-mode ids
Not recoverable reliably from these samples. Evidence:

| id | uses | channels (flags) | seen on layers named |
|---:|---:|---|---|
| 25 | 29 layer, ~500 in action groups | all channels (default entry of every action group); layer-level only on `Sharpen`, `HSL Perceptive` filters | filters; the default |
| 11 | 170 | Roughness 104, Base Color 54, others | `Fill layer N` |
| 2 | 103 | Base Color 38, Normal 38, Roughness, mixed | `Latex Black`, `Noise 01`, `Steel`... |
| 15 | 31 | Base Color | `Fill layer N` |
| 9 | 24 | Base Color | `Fill layer N` |
| 10 | 21 | Height 16, Roughness, others | `Edges`, `Paper`, `Soil - Mixed` |
| 4 | 3 (+92 in mask groups) | | `Color Variation`, `AO Dirt`, `Edges` |
| 23, 29, 0, 1, 13 | 3, 1, 2, 2, 1 | | `Dust` (23); `AO Dirt` (29) |

What is likely: 25 is "Normal" (it is the default of every group, and the only mode on filters); 1 on Height and 2 on
Normal for the `Steel Stained` folder match Painter's defaults (Height: add-like, Normal: normal-map combine) but
2 also appears on Base Color, so 2 cannot be a single mode named from the Normal channel. A table needs ground truth.
The cheapest way: the owner builds one Painter project with a uniform base layer and one uniform layer per blend mode
(and the three normal-map modes on a Normal channel), saves it, and the importer reads ids plus the flattened
texture cache; the mapping then follows from the pixel values (a few dozen lines). Searching the 11 projects for stacks
simple enough to test this found none (no stack where every contributing layer is an unmasked uniform fill).

## 5. Plan

### Tier 0, done
Flattened per-channel result as one paint layer, baked maps (`docs/spp-research.md`).

### Tier 1: reconstruct the stack from names, validate against the flattened result (cheap)
Parse is done (`LayerInfo` carries names, visibility, blends, sources; extend it with tweaks, mask actions and
Levels). Build SubCraft layers:
* folders and layers with names, visibility, opacity, mask presence;
* uniform fills, Levels, Invert; masks as effect stacks (Fill, Generator, Levels, Filter, Image);
* built-ins mapped **by canonical graph name** (table in section 3, parameters from `tweaks`, ranges from the XML),
  approximate ones flagged "approximated" in the layer name/notes;
* every unmapped layer (warp, brick, stripes, strokes, decals, procedural materials, unknown names) is kept as a
  **named, disabled placeholder** so the structure is visible, and the flattened layer stays at the bottom
  so the model never looks wrong. A switch "use reconstructed stack" and a computed per-channel error (composite
  vs. texture cache; the importer already has both) tell the artist how faithful the reconstruction is. If the error is
  high the stack stays hidden by default.
Effort: about 1.5 to 2 weeks (mapping table, parameter calibration for blur/sharpen/HSL/dirt/edges, tests, UI notes);
blend ids need the calibration project first. Risk: low for structure, medium for look (approximations), and it never
makes the result worse because of the flattened base and the error gate. Painted mask strokes stay out of it.

### Tier 2: exact where the data is in the file (small, high value)
* Extract the embedded JPEG of bitmap-based graphs (grunges, steel bitmaps, brush tips) and rebuild them as Image fill/mask + Levels:
  exact for 32 graphs and 171 uses, about 3 days. Needs a decision on copyright (section 6).
* Replay of brush strokes as mask/paint rasters through SubCraft's brush engine using the stroke samples and the brush tip
  JPEGs: 1 to 3 weeks, **approximate** (different engines), only worth it for mask strokes that matter (1448 in the samples,
  1091 of them in one file).
* Calibrated noise approximations for the `noise_*`/`ratio_*` family: 1 to 2 weeks, looks similar, never identical.
* Missing building blocks that recur: noise-driven `Warp` filter, brick/stripes/shape patterns, `UV border distance`: 1 to 2 weeks.

### Tier 3: interpret the compiled `.sbsasm` graphs with our own evaluator (heavy)
Decode five format versions, build a node/opcode catalogue by experiment against the 136 samples, implement the atomic nodes
(blend, blur, levels, gradient map, histogram ops, noise families, FX-map, 3D/mesh-map sampling) so that outputs match the cached
flattened results. Effort: months for a small team's worth, **risk very high** (undocumented, versioned, noise algorithms must
match, only the content of 11 files to test against, every new library graph may use unseen nodes), and the legal question below.
Not recommended before Tier 1 and 2 show where the remaining error actually is. A middle path is the same experiment on a handful of
simple graphs (`blur`, `sharpen`, `clamp`, `hsl_perceptive`, which are 1 KB each) to learn the opcode set cheaply (about 1 week) and
decide then.

## 6. License and legal flags for the owner to decide
* The `.spp` and `.sbsar` formats are proprietary (Adobe / Allegorithmic) and undocumented. What the importer already does (HDF5, Painter
  document, JPEG 2000 tiles, LZ4) reads the user's own project data for interoperability. The Substance 3D Painter and Substance engine
  terms generally forbid reverse engineering the software; whether reading file formats for interoperability is covered
  depends on jurisdiction (EU: software interoperability exception) and on the exact EULA text, which I have not seen and which
  is not legal advice. Re-implementing the **Substance engine** (Tier 3) is a larger step than reading project files: it recreates Adobe's
  product behaviour from its compiled content, and a strict reading of the EULA treats that as reverse engineering.
* **Content ownership:** the grunge bitmaps, steel bitmaps and brush tips inside the `.sbsar` files are Adobe/Allegorithmic
  content shipped with Painter's starter assets (and Substance Source items under their own licenses). Using them locally to render the
  user's own project is one thing; **bundling or exporting them from SubCraft** (for example saving extracted grunge JPEGs into SubCraft
  projects that are shared) is redistribution. Recommended: extract at import time into the user's project only, never ship them, and
  say so in the import notes. Same for the 80/80 starter pack: unaffected, those are SubCraft's own.
* Trademarks: use "Substance Painter project (.spp)" as a file-format name only, no Adobe branding.
* The DBAL-A2 sample is under a non-commercial, private-research license: it was only parsed for this inventory; keep it out of tests and docs.
