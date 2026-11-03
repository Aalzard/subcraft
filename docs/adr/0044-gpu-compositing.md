# ADR-0044 — GPU compositing

Status: Accepted (2026-11-03)

## Context

The CPU compositor (`crates/compositor`) evaluates a texture set tile by tile: layer content,
masks and blends, f32 per texel, stored as f16 in a `TiledImage` per channel. For a whole-set
change (open a project, toggle a layer, change a layer's opacity or blend mode) every tile of
every channel is evaluated again. The blend itself is cheap arithmetic on 64x64 tiles and maps
well to a GPU; the expensive parts of a heavy project (procedural fills, mesh-map generator
masks, blur-type filters) are not blends.

## Decision

**Optional backend, CPU stays the reference.** `compositor` (no wgpu) defines the trait
`GpuCompositeBackend` and a process-wide registry (`register_gpu_compositor`), the same pattern as
GPU baking (ADR-0039). `crates/render` implements it with one WGSL compute pass per channel on a
headless device of its own. The UI registers it at start-up. The preference "Use GPU for
compositing" (`set_gpu_compositing_enabled`) switches it; it is **on by default** and applies to texture sets of
2048² and larger (`DEFAULT_MIN_RESOLUTION`), where it measured faster than the CPU for whole-set edits; at 1024² the
CPU wins. Software adapters
(llvmpipe, WARP) are refused unless `SUBCRAFT_GPU_COMPOSITE_SOFTWARE` is set, because the CPU
path beats them. Any error, a missing adapter or a lost device sends the affected tiles to the
CPU path; stored data is never touched by the GPU path except through the same
compare-and-store step the CPU uses.

**What the GPU does.** It blends: the layer/folder stack walk, opacity, masks, isolated and
passthrough folders (nesting up to 4), all 32 blend modes (separable, HSV and normal-map modes
ported 1:1 from `blend.rs`), and the per-pixel content effects Levels, Contrast, Threshold, Clamp,
Histogram Scan and Histogram Range, on layers and on folders.

**What stays on the CPU, per layer and tile.** The compositor walks the same tree and prepares
each tile's sources on the CPU in parallel: a uniform fill becomes a constant; an unmodified paint
raster or a plain mask raster becomes a reference to the stored tile; everything else (procedural,
bitmap and mesh-map fills, generator / image / levels mask stacks, part masks, paint or fill
effects, gradient maps, HSL, differently sized rasters) is evaluated by the existing CPU code and
passed as an f32 block. Texture sets that contain neighborhood filters (blur, sharpen, median,
warp, ...) are not supported (they read other tiles) and use the CPU for the whole set. Tiles with a resumable CPU layer-cache prefix stay on the CPU, and when more than 25 % of the sources of the first batch are CPU-evaluated blocks the rest of the set goes to the CPU. Edits
touching fewer than 64 tiles (a brush stroke) stay on the CPU, which answers sooner than a GPU
round trip.

**Memory.** No dense per-layer GPU textures. Paint and mask tiles are uploaded as the stored f16
tiles into a pool of 32 KiB slots spread over up to four storage buffers (each at most the device's
`maxStorageBufferBindingSize` and 512 MiB; 2 GiB in total by default, `SUBCRAFT_GPU_POOL_MB`, grown on demand), so a
4096² working set stays resident between edits. Slots are found again by
the identity of the `Arc<Tile>` the cache holds; copy-on-write makes an edited tile a new key, so
a stale tile can never be read. Least recently used slots are evicted. Work is cut into batches of
at most 256 tiles and a CPU-data budget (about 192 MB of f32 blocks); a batch the GPU cannot hold
is split and, at one tile, falls back. Output is f16 `rgb + alpha` per texel and channel, read
back per batch and stored with the CPU's compare-and-keep logic, so only really changed tiles are
reported dirty.

**Equivalence.** Both paths compute in f32 and round to f16. The GPU is not bit-identical (fused
multiply-add, pow / sqrt / division rounding), so tests accept `|gpu - cpu| <= 2^-9 * max(1,
|value|)` (about two f16 steps near 1.0) with no more than 0.1 % of values beyond one such unit,
no NaN, and exact repeatability on the same device. Measured values, including CPU vs GPU timings, are in `docs/progress.md`.

## Consequences

Whole-set edits of paint-, uniform-fill- and mask-raster-heavy stacks no longer evaluate every
texel on the CPU. Stacks dominated by procedural fills or generator masks gain little, since that
work is still CPU work (it is only not repeated for the blend). Sets with blur-type filters are
unchanged. Upload and readback cost is real and is reported by `gpu_composite_bench`.

Keeping CPU-evaluated blocks (procedural fills, generator masks) resident on the GPU between edits
was implemented and measured, then reverted: it sends procedural sets to the GPU, which then
re-blends every tile instead of resuming from the CPU per-layer prefix cache, and was several times
slower (45.7 s vs 17.5 s for a 4096² toggle). Later steps, if measurements justify them: GPU
procedural patterns, letting GPU results fill the CPU prefix cache, and a neighborhood-aware path
for blur-type filters.
