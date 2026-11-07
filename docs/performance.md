# Performance notes (branch `perf-overnight`)

## How to measure

`cargo run --release -p subcraft-app --example pen_stroke_bench -- [res] [layers] [brush_px] [textured 0|1] [frame|event|batch] [mid|top]`
now also prints **stage wall time** (`bin`, `raster`, `accumulate`, `write`) from
`subcraft_paint::stroke::profile` (always-on counters, one `Instant::now` per stage per call).
Other benches already in the repo: `big_project_bench` (full composite, incremental edits, open
project, save/load, bake, layer edits, interactive feedback), `perf_slider`, `bake_bench`.

**Measurement caveat (important):** these numbers were taken on a PC where 20+ rustc/cargo processes of
other sessions were running. Identical runs of the same binary varied up to 5x (12 ms vs 87 ms
per call at 2048²). Only best-of-N from alternated A/B runs, with `RAYON_NUM_THREADS=1`, is meaningful.
Re-run on a quiet machine before trusting any single figure.

Also: building into the shared `sc-shared-target` from several worktrees produced stale artifacts
(an example compiled against another worktree's older `subcraft-paint`/`subcraft-app` rlibs, because
workspace members get path-independent hashes). This branch builds into its own target dir.

## Baseline (pen_stroke_bench, 10 layers, 300 px soft brush, batch mode, 800 ms stroke)

| case | ms per stroke_to call |
|---|---|
| 2048², best of several runs, 16 threads | 5.1 |
| 2048², best of 6, 1 thread | 12.4 |
| 4096², best of several runs, 16 threads | ~17-22 (p95 60-100) |

Stage split at 2048², 1 thread (best run, total 1555 ms for the stroke): bin 4, raster (first touch of
a tile) 360, accumulate (footprint evaluation) 452, write (compute_tile + f16 encode + store) 685.
So on the CPU, `write` is the largest stage, then footprint evaluation, then first-touch raster.

## Tried and rejected

* Hoisting the stamp's `sin_cos` and the per-triangle tangent fold frame out of the per-sample
  footprint loop (bit-identical output). Best-of-6 at 2048², 1 thread: 13.1 ms vs 12.4 ms base, i.e.
  no measurable gain; reverted.

## Not done (needs a quiet machine and a decision)

* 60 fps for a 300 px brush at 4096² is not reachable by tuning the CPU kernel: per stamp it is
  still rasterize + footprint + f16 tile write over 40-110 tiles. Options that change the design:
  GPU stamping into a GPU tile cache with deferred readback; or a low-resolution live preview while
  drawing (exact result at stroke end). Both are multi-day work and need owner sign-off.
* Benchmarks for 20/50-layer stacks, orbit at 2048, texture-set switch and export were not run.
