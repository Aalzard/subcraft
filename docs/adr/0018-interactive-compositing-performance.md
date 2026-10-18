# ADR-0018 — Interactive compositing performance

Status: Accepted (2026-10-18).

## Context

The owner reported that dragging generator and material sliders was "really slow, even at
1000 resolution", and that lowering the viewport resolution did not help. Measurements
(`crates/app/examples/perf_slider.rs`, the demo stack: procedural wood + moss fills, a
directional generator with noise breakup, a paint layer) showed the cost was CPU compositing,
not rendering: every slider tick re-evaluated every tile of every layer and channel.

| per generator-level step | 1024² | 2048² | 4096² |
| --- | --- | --- | --- |
| before | 260–386 ms | 3.2–5.4 s | — |
| after (synchronous full update) | ~10 ms | ~37 ms | ~145 ms |

Where the time went, in order of size: the Windows system heap serializing the compositor's
parallel per-tile buffers; recomputing procedural fields (fractal noise) that the edit could not
have changed; a single-threaded f16 encoding pass; uploading every tile of every channel to the
GPU; and the 3D and 2D panels each compositing again within the same frame.

## Decision

1. **mimalloc** is the desktop global allocator (`apps/subcraft`). The system heap made the
   parallel compositor 7× slower. Libraries stay allocator-agnostic; the web build keeps the
   default.
2. **Procedural field cache** (`materials::sample::field`): each tile geometry keeps the
   procedural fields evaluated on it, keyed by every parameter bit (pattern kind/scale/seed/
   balance/contrast/invert + projection). Colors, blending, masks, levels, generator level/
   softness/amount reuse them. Results are bit-identical to evaluating again (tested). Bounded
   to 8 fields per tile and a 768 MB process budget (beyond it fields are computed uncached).
3. **Exact noise speedup**: gradient noise remembers lattice gradient directions per thread
   (direct-mapped by hash). Same `sin`/`cos` values, so patterns and existing projects are
   unchanged (bit-identity tested); cold fields are ~2× faster.
4. **Parallel tile writes** straight into the composite, and **change detection**: a tile whose
   re-evaluated f16 texels are identical keeps its storage and is not reported. The renderer
   uploads only changed tiles per channel (a Moss mask drag uploads Base Color/Roughness where
   the mask actually changed, not five full channels).
5. **Frame budget**: the UI composites at most ~10 ms per frame (shared by all panels in the
   frame); remaining dirty tiles stay pending and are drained round-robin on the next frames,
   always against the newest document, so the result converges exactly. Small fresh changes
   (brush strokes) are evaluated first. Export, bake, saving and tests use the full update.
6. Layers fully masked out in a tile skip their content evaluation (exact).

## Consequences

* Real app while a generator value changes every frame (`subcraft --profile-edits <file>
  [resolution]`, Windows, RTX 4060, 16 CPU threads, after the first full composite):

  | | 1024² | 2048² | 4096² |
  | --- | --- | --- | --- |
  | frame interval p50 / p95 | 15.2 / 26.7 ms | 15.7 / 25.3 ms | 17.0 / 24.8 ms |
  | composite + upload p50 | 12.1 ms | 12.7 ms | 14.4 ms |
  | model fully refreshed after the last edit | 5 ms | 73 ms | 0.9 s |

  Before this work a single 1024² step took 260–386 ms of blocked UI before upload.
* Cold fields cost real time once: opening/creating a 4K stack computes every procedural field
  (several seconds of CPU on 16 threads); the UI stays usable but runs at ~25 fps meanwhile,
  because one cold 64² tile cannot be split across a frame.
* Parameters that change a procedural field (pattern scale/seed/balance, projection tiling/
  rotation/offset, generator noise settings) recompute that one field: ~25 ms at 1024².
* Remaining big levers, not done: GPU compute kernels for fields and blending; compositing on a
  worker thread so cold fields never cost a frame; mip/resolution-reduced previews during drags;
  staged caching of the accumulation below an edited layer.
