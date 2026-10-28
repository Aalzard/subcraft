# ADR-0039 — GPU baking

Status: Accepted (2026-10-28)

## Context

The CPU baker (`crates/bake`) is slow on big meshes. Almost all of the time is spent in the
ray-based maps: ambient occlusion (default 32 rays per texel) and thickness (16 rays per texel),
each a BVH walk per ray. Bent normal uses the same rays as AO. Curvature is a neighbourhood
query per texel and costs less. Normal, world normal, position, ID and height are one lookup per
texel and are already cheap.

## Decision

**Which maps.** First AO and thickness run on the GPU. Curvature follows only if the
equivalence against the CPU result is good (it is a separate step). Everything else stays on the
CPU, as does the high-poly projection, UV rasterisation, gutter padding and the file writing.
The CPU baker remains the reference and the fallback. Bent normal stays on the CPU for now.

**Where the code lives.** `bake` (no wgpu) defines a small trait, `GpuBaker`, and a process-wide
registry (`set_gpu_baker_factory`). `crates/render` implements it with wgpu compute shaders (WGSL
only, no ray-query extension) on its own headless device, so the viewport device is never
stalled. The UI/app start-up registers the factory. `BakeSettings::use_gpu` (default true,
driven by the preference "Use GPU for baking") asks for it. Any error, a missing adapter, an
unsupported size or a lost device falls back to the CPU path for that map; the result reports
which path ran and how long it took.

**What the CPU still prepares per texel.** The CPU path already produces, for every covered texel,
the surface point, the detail point/normal and the seed `hash01(index, k)`. Those are passed
unchanged, so coverage, gutters and sample rotation match exactly: per texel one record of
`origin + rotation (vec4)`, `direction axis (vec4)` and `offset normal (vec4)` = 48 bytes. The
WGSL builds the same Hammersley/cosine directions from `rotation` (same orthonormal frame as
glam's `any_orthonormal_pair`).

**Mesh and BVH layout.** `Bvh::flatten()` exports the SAH tree as two plain arrays: nodes of 8
words (`min.xyz, left_first, max.xyz, count`) and triangles re-ordered into leaf order so a leaf
reads `v0, e1, e2` (3 x vec4) directly with no index indirection. They are two read-only storage
buffers uploaded once per bake. Traversal is a 64-entry stack in a private array (the CPU
version falls back to a heap stack on deeper trees; the GPU refuses trees that need more and
falls back to CPU). Triangle test is the same Moller-Trumbore as the CPU, double sided.
Buffer sizes are checked against `max_storage_buffer_binding_size` and `max_buffer_size` before
upload; too big means fall back, never panic.

**Chunking (TDR safety).** A dispatch covers a range of texels and a range of rays
(`texel_start..texel_end`, `ray_start..ray_end`). A per-texel accumulator buffer holds the running
sum (AO: occluded and counted rays; thickness: summed distance), so rays-per-pass can be
smaller than the sample count. The host measures each submit (submit + wait) and adapts the
number of texel x ray work items so one dispatch targets about 100 ms and never more than
about 250 ms, starting small. Cancel is checked and progress advanced between dispatches, exactly
like the CPU chunk loop. Texel records are uploaded per chunk, so memory is bounded
(about 48 B x chunk, not x 16M texels at 4096).

**Accuracy.** GPU floating point (fused multiply-add, different sin/cos) differs in the last
bits, so a ray grazing an edge can flip. Equivalence is tested with a mean absolute error and a
99th percentile bound, an identical coverage mask, no NaNs, and exact repeatability on the same
device. The GPU is not bit-identical to the CPU and is not meant to be.

## Consequences

Large meshes bake much faster where an adapter exists. CI (llvmpipe) runs the same shaders
slowly but correctly; tests skip with "no adapter" if there is none. The shaders add no
dependencies. Bent normal, curvature and the high-poly projection stay on the CPU until
measurements justify moving them.
