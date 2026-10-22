# ADR-0020 — Albedo filtering in document evaluation and display

Status: Accepted (2026-10-19)

## Decision

Store `TextureSet.albedo_filter` as Bilinear (default) or Nearest. The owner explicitly requested
both viewport and exported textures: a GPU-only switch would leave low-resolution imported
images blurred during CPU fill projection into the working texture.

`image::TextureFilter` defines reconstruction independently of address wrapping/color decoding.
Nearest selects the containing texel at normalized UVs, wraps/clamps integer coordinates, then
decodes color normally; alpha is never color-transformed. Existing sampler/resampler APIs stay
bilinear for callers that do not opt in.

The production fill and content-fill evaluator uses the texture-set filter only for Base Color
bitmap channels. Procedurals, masks and other channels retain their behavior. Textured brush
strokes capture the setting at begin; existing raster strokes are not regenerated. Changing
working resolution resamples Base Color paint rasters (including content effects/folders) with
the selected filter; masks/data channels keep their previous bilinear resampling. Already blurred
paint cannot be restored by changing this setting.

Compositing caches treat a filter edit as an input change, so synchronous export and background
viewport evaluation converge to the same result. Thumbnail snapshots include the filter in
their invalidation key. `Command::SetAlbedoFilter` edits only the requested set, creating one
undo step when changed and none for a no-op.

Render owns two persistent samplers. An additional Base Color sampler binding is selected per
material slot; changing it recreates that bind group only, without texture uploads or pipeline
compilation. Material and unlit Base Color views share it. Other PBR/data/environment sampling
is unchanged. The 2D Base Color display uses the same choice for magnification.

UI dropdowns sit beneath Resolution in the 3D toolbar and Texture Sets panel, reading the same
document field and dispatching the same command. Narrow 3D docks expose it in the View menu.
This is texture-set document state, not a global viewport preference or brush preset field.

Project schema **6** writes `albedo_filter`; migration from schemas **1–5** supplies Bilinear
when the field is absent. The explicit schema/default and regression test preserve old files.
Older published readers that only support schema 5 reject new schema-6 saves rather than
silently losing the selection. Existing layer/resource/paint serialization stays unchanged.

## Meaning and limits

Nearest changes image reconstruction. Soft brush coverage, opacity/layer blends and triplanar
axis blending still mix colors deliberately; UV/planar projection is suitable for exact palettes.
Exported pixels preserve reconstruction, but PNGs do not configure a game engine's runtime
sampler: choose nearest/point sampling there as well. This does not add PS1 geometry wobble,
affine mapping, dithering, palette quantization or automatic mipmaps.

Numeric acceptance covers texel boundaries, wrapping/clamping, color/alpha, raster resampling,
fill/composite/export values, data-channel isolation, undo/redo and schema migration. Textured
stroke acceptance exercises both filters through UV/planar/triplanar projection. Real GPU
checks compare nearest palette values and numeric bilinear interpolation, material/unlit views,
data-channel isolation and toggling without uploads. Native screenshots are in progress.md.
