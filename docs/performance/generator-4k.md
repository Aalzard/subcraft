# Generator sliders on a 4096² texture set (2026-11-08)

Owner report: changing any generator's settings on a 4K texture set is really slow.

## How it was measured

* `cargo run --release -p subcraft-app --example generator_4k -- 4096 [letters]`
  (`crates/app/examples/generator_4k.rs`): the sample crate at 4096², curvature + AO baked
  (`SUBCRAFT_BENCH_BAKE`, 4096 unless noted), a smart-material-like stack: procedural base metal,
  red paint, and one masked fill per generator, bottom to top: Dirt, Edges (natural wear),
  Concave, Directional, Position Gradient, and a second Dirt with the micro effect (which makes
  the set use the neighborhood graph evaluator, as any blur/filter layer does). Every generator
  has its own breakup seed, as in a real stack. Three measurements per generator:
  * **sync**: one Level step brought exactly up to date (blocking `update_composites`);
  * **preview**: the 512² quick composite and putting it on the 4K composite;
  * **drag**: 30 Level steps at 60 Hz through the background worker exactly as the UI drives it:
    *visible update* = time from a step until a publication covering the whole image (a quick
    preview) or the exact result; *settle* = from the last step until the exact 4K result.
  `SUBCRAFT_BENCH_HASH=1` prints a hash of every composite texel bit (exactness across builds),
  `SUBCRAFT_COMPOSITE_PROFILE=1` the graph node time, `SUBCRAFT_BENCH_SAVE=<file>` saves the stack.
* Real app: `subcraft <saved stack> --profile-edits out.txt` with
  `SUBCRAFT_PROFILE_GENERATOR=dirt` (any part of a generator label) drags that generator's Level
  every frame through the normal command, compositing, GPU upload and rendering, and reports frame
  intervals, edit-to-visible-update latency, upload time and full-res settle.
* Machine: 16 threads, RTX 4060, 64 GB. **Other sessions' builds and tests were using 8 to 12 of
  the 16 cores during most runs**, so absolute numbers are noisy (repeat runs differ by up to
  30 %); before/after pairs were run back to back.

## Where the time went (before)

| 4K, one Level step, exact | sync | settle after a drag |
|---|---|---|
| Dirt (bottom of the stack) | 4.9 to 6.9 s | 8.3 s |
| Edges (natural wear) | 2.5 to 3.4 s | 4.6 s |
| Concave | 2.4 s | 1.9 s |
| Directional | 1.0 s | 1.2 s |
| Position Gradient | 0.7 s | 1.0 s |
| Dirt + micro (top) | 0.5 s | 0.85 s |

(maps baked at 1024 for this first run)

1. **Procedural field cache thrash (the main cost).** Generator breakup noise, fine grain, edge
   chips and the edge-distance field are cached per tile, at most 8 fields per tile with plain
   least-recently-used replacement. This stack needs 13 per tile, read in the same order on every
   step, which is the worst case for LRU: every read evicted the field the next layer was about
   to read, so *every* generator recomputed its fractal noise on every step (about 3.3 ms per
   generator tile, 80 s of CPU per step at 4K). The cache sat at exactly 4096 tiles × 8 × 16 KB
   = 512 MB.
2. **Every generator above the edited one is re-evaluated** (blending is cached below the edited
   layer by the prefix cache, not above), so editing a generator low in the stack costs more.
3. **Quick previews were slow to build and too big to upload.** While a slider moves the worker
   shows a 512² composite stretched over the 4K one. Computing it took 20 ms; stretching it over
   all 4096 tiles of every channel took 165 to 190 ms (per-texel f32 round trips, a serial copy
   of ~650 MB of tiles, not cancellable), and every tile of every channel was then re-uploaded to
   the GPU. During a drag the image so refreshed about 5 times a second.
4. Mesh maps baked below the set resolution are sampled bilinearly per texel on every read
   (about a third of a Dirt mask's cost with 1024 maps on a 4096 set). Not changed; with maps
   baked at the set resolution this is a tile copy.

## Fixes

1. `materials::sample::field` (`crates/materials/src/sample/field.rs`): 24 fields per tile, a
   budget sized from the machine's memory (a twelfth, 768 MB to 4 GB; set once by
   `subcraft_app::Scene::new`), and scan-resistant replacement: a field is only replaced after it
   went unused for 48 reads of its tile, so a working set larger than the cache keeps hitting on
   what it holds instead of missing on everything. Values are computed by the same code, so
   results are bit-identical (composite hash `68f9fd3373067faf` before and after).
2. Quick previews (`crates/compositor/src/lib.rs`, `worker.rs`): tiles are stretched by copying
   the stored f16 texels in parallel straight into new tiles (same values as before, tested);
   when the worker goes idle it keeps a 512² composite of the exact state (the baseline), and a
   preview then only replaces the tiles, per channel, whose low-resolution texels differ from it.
   Everything else keeps the crisp exact tiles and is not uploaded again. Exact refinement and
   convergence are unchanged.
3. Previews are no longer cancelled by the next snapshot (`worker.rs`). A slider sends one
   snapshot per frame and a 4096² preview takes ~20 ms, so every preview was cancelled and
   restarted: **during a drag nothing was shown until the pointer paused** (the real app's
   baseline below reports no visible update at all while dragging). A preview in progress is now
   finished and the newest snapshot is previewed next; when a newer snapshot is already waiting,
   the worker skips the exact slice in between (that snapshot dirties those tiles again).

## After

Benchmark, same stack and settings, exact result unchanged (hash `68f9fd3373067faf` with 1024
maps before and after):

| 4K, one Level step, exact (sync) | before (1024 maps) | after (1024 maps) | after (4096 maps) |
|---|---|---|---|
| Dirt (bottom) | 4.9 to 6.9 s | 1.22 to 1.42 s | 0.88 to 1.35 s |
| Edges (natural wear) | 2.5 to 3.4 s | 0.92 to 1.08 s | 0.68 to 0.96 s |
| Concave | 2.4 s | 0.93 to 1.12 s | 0.67 to 1.21 s |
| Directional | 1.0 s | 0.62 to 0.69 s | 0.49 to 0.70 s |
| Position Gradient | 0.7 s | 0.48 to 0.53 s | 0.37 to 0.45 s |
| Dirt + micro (top) | 0.5 s | 0.36 to 0.41 s | 0.23 to 0.34 s |

| 4K quick preview | before | after |
|---|---|---|
| 512² composite of the whole stack | 20 ms | 20 to 30 ms (unchanged) |
| putting it on the 4K composite | 165 to 190 ms, every tile of every channel | 2 to 11 ms, 46 to 3090 of 20480 channel tiles |

Drag through the background worker, 30 Level steps at 60 Hz, 4096 maps (visible update = until
a whole-image publication; machine under load from other sessions):

| generator | visible update p50 / p95 / max | settle after the last step |
|---|---|---|
| Dirt | 39 / 81 / 100 ms | 1.65 s |
| Edges (natural wear) | 38 / 94 / 99 ms | 1.35 s |
| Concave | 36 / 73 / 94 ms | 1.04 s |
| Directional | 37 / 73 / 85 ms | 0.92 s |
| Position Gradient | 43 / 91 / 121 ms | 0.76 s |
| Dirt + micro (top) | 42 / 90 / 106 ms | 0.57 s |

(The benchmark's old "visible update" counted any slice of exact tiles as visible; the numbers
in the first table of this file are therefore not comparable and are not repeated here. With
1024 maps settle is 0.9 to 2.4 s.)

Real release app (`--profile-edits`, the stack above with 4096 maps, 120 frames of Level edits;
edit to visible update = to the first frame that installed a whole-image publication):

| run | frame interval p50 / max | edit to visible update p50 / p95 | full-res settle |
|---|---|---|---|
| Dirt, before (2 runs) | 17.7 / 47.8 ms, 14.2 / 49.3 ms | **nothing shown while dragging** | 6.4 s, 4.9 s |
| Dirt, after (4 runs) | 14.5 / 38.4, 14.7 / 44.9, 17.4 / 42.8, 16.4 / 37.3 ms | 43 / 104, 69 / 548, 18 / 77, 34 / 74 ms | 1.3 s, 5.4 s, 1.0 s, 1.4 s |
| Edges, before / after | 15.7 / 51.9 ms; 14.5 / 285.5 ms | nothing shown; 215 / 401 ms | 8.4 s; 6.5 s |
| Directional, before / after | 31.7 / 71.3 ms; 13.5 / 62.5 ms | nothing shown; 115 / 288 ms | 14.1 s; 1.1 s |
| Dirt, final build | 15.3 / 48.7 ms | 22 / 113 ms | 1.4 s |
| Edges, final build | 13.2 / 37.5 ms | 27 / 67 ms | 1.5 s |

The second Dirt run and the Edges / Directional pair ran while other sessions' compiles used about
8 of the 16 cores (measured); the quieter Dirt runs show the expected 1 to 1.4 s settle with
~75 ms p95 visible updates; the final build (baseline skipping for unchanged sets added) settles Dirt and Edges in about 1.5 s. The UI thread never blocks (frame interval max under 65 ms except one
285 ms frame in the loaded Edges run).

## Not done / still open

* Layers above the edited generator are still re-evaluated on every step (their masks are not
  cached); a per-layer mask cache (about 64 MB per masked layer at 4K) would bring a bottom-of-stack
  edit close to a top-of-stack one.
* Mesh maps baked below the set resolution are resampled on every read (could be cached like
  procedural fields).
* Unrelated, seen in every app log: the viewport's `grid pipeline` fails wgpu validation every frame
  (sample count 4 pipeline in a sample count 1 pass).
