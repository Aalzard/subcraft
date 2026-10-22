# ADR-0036 — Incremental viewport material mipmaps

Status: Accepted (2026-10-20)

## Context

Imported 4K PBR materials show fine grain and unstable highlights when drawn much smaller
than their texture resolution. Material GPU textures had only mip zero. Mesh MSAA smooths
silhouettes but cannot average subpixel material detail. Normal and height detail are
particularly sensitive to this undersampling.

## Decision

Keep authoritative linear `TiledImage` composites, imported resources and exports unchanged.
Generate a full GPU mip chain for viewport material channels in `render`. A render pass
area-averages each level into the next, including odd-sized textures. R16F, RG16F and RGBA16F
use cached pipelines; views and source bind groups are cached with each channel texture.
Encoded normal vectors are averaged without normalizing the mip texels; the shading stage
normalizes its final combined normal as before.

The material sampler uses trilinear filtering. The explicit Nearest albedo mode clamps LOD
to zero, retaining the owner's pixel-art workflow. Baked mesh-map textures retain their
existing separate upload path. Mip generation does not change project schema or undo.

Full uploads preserve sparse initialization: clear mip zero to the image fill, upload only
allocated tiles, then generate the chain. Partial uploads propagate the union of dirty tile
bounds down the chain with scissor rectangles. Empty dirty lists do no mip work. Queue order
ensures mip generation sees uploaded texels before sampling. No GPU readback/wait is added
to interactive rendering.

## Consequences and checks

Material GPU storage increases by approximately one third for power-of-two textures. Each
changed channel has one pass per lower level; scattered edits may cover a broad union.
Numeric GPU tests cover averaging, odd sizes, formats, incremental/full equivalence, cleared
tiles and unchanged mip-zero data. A minified checker acceptance test covers camera offsets,
linear-space averaging and the intentional Nearest exception. A bounded 4K update profile
reports upload + mip generation + completion time; it does not establish interactive FPS.

Native before/after captures use an isolated data directory, the real PBR importer and the
normal viewport. The owner's unsaved project is unavailable, so representative materials
are evidence, not an exact reconstruction of that scene. No source PBR map is made smoother
or less glossy merely to hide the rendering defect.
