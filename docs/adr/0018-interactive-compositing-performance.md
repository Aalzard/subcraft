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
5. **Background desktop compositing** (session 70): `CompositeWorker` owns caches and a private
   Rayon pool with `max(1, available CPUs - 1)` threads. The UI submits immutable COW snapshots;
   a single pending slot replaces superseded edits. Invalidation survives replacement. The
   worker evaluates approximately 8 ms slices, publishes shared composites and the union of
   changed tile indices, then checks for a newer snapshot. Cold tiles can exceed a slice but
   no longer execute on the UI thread. The private pool avoids queueing brush work behind
   compositor jobs. Platforms without threads retain the shared ~10 ms inline frame budget.
   Dirty work drains round-robin against the newest snapshot, with small fresh edits first.
   `Session::update_composites` waits for the exact latest result for export/sampling/tests;
   save persists the authoritative document/resources/maps directly, never a viewport image.
6. Layers fully masked out in a tile skip their content evaluation (exact).
7. **Sparse GPU initialization**: full channel uploads clear the GPU texture to the exact f16
   fill value, then upload allocated tiles only. RGB fill pads alpha to 1 just like tile uploads.
   Clears are submitted before queued tile writes. Partial updates retain changed-tile uploads.
   This avoids hundreds of MB of implicit fill uploads while a new 4K stack warms up.
8. Existing synchronous composites remain displayed when their caches move to the worker.
   A publication generation lets UI consumers detect composites installed by export or sampling
   between frames; otherwise those callers could consume notifications and leave stale GPU tiles.

## Consequences

* Session 69, before the background worker, while a generator value changes every frame (`subcraft --profile-edits <file>
  [resolution]`, Windows, RTX 4060, 16 CPU threads, after the first full composite):

  | | 1024² | 2048² | 4096² |
  | --- | --- | --- | --- |
  | frame interval p50 / p95 | 15.2 / 26.7 ms | 15.7 / 25.3 ms | 17.0 / 24.8 ms |
  | composite + upload p50 | 12.1 ms | 12.7 ms | 14.4 ms |
  | model fully refreshed after the last edit | 5 ms | 73 ms | 0.9 s |

  Before this work a single 1024² step took 260–386 ms of blocked UI before upload.
* Cold fields still cost CPU time once. Worker evaluation frees the UI thread; contention,
  first-use graphics work and upload/render costs can still lengthen frames. This is not a
  guarantee of zero dropped frames. Native profiling measures warm-up, generator edits and
  pattern-scale edits separately, including final convergence and chronological startup intervals.
* Parameters that change a procedural field (pattern scale/seed/balance, projection tiling/
  rotation/offset, generator noise settings) recompute that one field: ~25 ms at 1024².
* Splitting fields within a cold tile was tried by Claude at 1K/4K and reverted: tile-level
  parallelism already occupied the cores and there was no measurable gain. No nested field
  scheduler ships. This session inspected that the experiment was absent; it was not re-run.
* Remaining big levers, not done: GPU compute kernels for fields and blending;
  mip/resolution-reduced previews during drags;
  staged caching of the accumulation below an edited layer.

## 0.1.7 continuation measurements

Final release build, Windows, 4096² demo, `--profile-edits` (commands drive the real app loop;
no mouse input), raw report: `docs/performance/2026-10-18-native-4k.txt`.

| phase | frame p50 / p95 / max | final convergence |
| --- | --- | --- |
| first composite | 14.1 / 27.5 / 440.7 ms | 2104 ms total |
| generator drag | 14.6 / 33.9 / 94.6 ms | 315 ms after last edit |
| pattern scale drag | 15.1 / 28.6 / 29.2 ms | 576 ms after last edit |

The first two warm-up intervals were 351.9 and 440.7 ms; the longest was interval **2**.
Warm-up section maxima include upload 183.7 ms, 3D 121.9 ms, 2D 126.2 ms and Shelf 75.7 ms
(these are separate maxima, not a single-frame breakdown). This locates the outstanding large
warm-up outlier at startup; it does not remove first-use graphics costs or occasional later
GPU/upload/render spikes. Do not claim uninterrupted 60 fps.

`cargo run --release -p subcraft-app --example perf_slider -- 4096` also passed. Synchronous
generator steps: p50 157.2 ms / p95 180.6 ms; first full composite 1475.6 ms. Worker UI polling
p95 during generator editing / cold warm-up / scale editing: **0.6 / 0.2 / 0.3 ms**, excluding
GPU uploads and rendering. Warm-up converged in 2026 ms. Full log is adjacent to the native
report as `2026-10-18-perf-slider-4k.txt`.

The session-69 baseline above is retained from Claude's earlier measurements. No baseline
executable was found for a new paired native rerun; the continuation first checked Claude's
uncommitted worker build (14.6 ms generator median, 445.1 ms warm-up maximum), then the final
build above. Differences between those two worker runs are ordinary measurement variation,
not a claimed further frame-rate improvement.

Acceptance: five worker tests (exact convergence/changed uploads, latest wins, invalidation,
cache adoption, resolution/channel replacement), three Session tests (sync/async convergence,
retaining prior sync results, save/export while work is pending), a GPU viewport export test,
and sparse-vs-dense GPU equality. **414 workspace tests pass**, 0 failed/ignored; Clippy exits 0
with existing warnings. Visible native evidence: `docs/screenshots/release-017-layers.png`.
Not checked by hand in this continuation: pointer gestures, tablet input, a complex production
model, browser runtime, Linux runtime, or updating an installed copy end to end.
