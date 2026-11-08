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

## Live preview for expensive strokes (kept)

Reasoning for choosing it over GPU stamping first: it is a small change inside `Stroke` (no new GPU
paths, no per-GPU risk), the final texels do not depend on it, and it cuts the live cost of exactly the
case that hurts (big brush, big texture). GPU stamping stays the longer-term answer (below).

How it works (`paint::stroke::LivePreview`, `Stroke::flush_deferred`, called from `Session::end_stroke`):
the stroke estimates tiles touched per stamp; above 32 tiles it applies only every 2nd/3rd resolved
stamp while the pen moves and keeps the rest. On release the skipped stamps are applied in one batched
pass (one tile write each). Coverage accumulates as `1 - prod(1 - a_i)`, independent of order, so the
released stroke equals the exact one up to float rounding. Test
`live_preview_thins_stamps_but_the_released_stroke_is_exact` (max texel difference < 1e-3, fewer
stamps applied while drawing). Paint and Erase only; smudge, clone and 2D-view strokes are unchanged.
`SUBCRAFT_NO_STROKE_PREVIEW=1` turns it off (used for the A/B below).

Same binary, env switch, 10 layers, 300 px soft brush, batch mode, wall clock on a still-busy PC
(three alternated pairs each; ms per `stroke_to` call / end_stroke):

| case | off | on |
|---|---|---|
| 2048² (stamp touches < 32 tiles mostly) | 6.6 / 15.7 / 39.7 | 5.3 / 15.9 / 16.6 (no consistent change) |
| 4096² | 22.3, 29.1, 128 | 8.1, 9.6, 33.0; end_stroke adds 148-236 ms |
| 4096² frame p95 (ms) | 78.8, 105, 251 | 34.4, 44.4, 149 |

So at 4096² the work while drawing drops about 2.5-3x and the total including the release flush is
still lower (one write per tile instead of one per stamp), at the price of a 0.15-0.25 s pause on pen
up and a preview that lags the pen by up to 2 stamps and updates every 2nd-3rd frame. A frame that
does apply a stamp still costs 20-30 ms at 4096², so this is smoother, not 60 fps.

## Not done (needs a quiet machine and a decision)

* 60 fps for a 300 px brush at 4096² is not reachable by tuning the CPU kernel: per stamp it is
  still rasterize + footprint + f16 tile write over 40-110 tiles. Options that change the design:
  GPU stamping into a GPU tile cache with deferred readback; or a low-resolution live preview while
  drawing (exact result at stroke end). Both are multi-day work and need owner sign-off.
* Prototype GPU stamping behind a feature flag (the next step; not started).
* Benchmarks for 20/50-layer stacks, orbit at 2048, texture-set switch and export were not run.


## Generator micro effect (2048, SubCraft_base, 2 texture sets, Dirt + Edges layers)

`cargo run --release -p subcraft-app --example micro_sheet -- <prefix> 2048` (mimalloc, 16 threads, release). The loop recomposites
everything after changing both generators; the first state of a round pays the noise fields of a new seed, the second reuses them.

| state | first of round (new seed) | second of round (noise cached) |
|---|---|---|
| micro OFF | 0.44, 0.39 s | 0.175 s |
| micro ON | 0.34 s | 0.156, 0.172 s |

So with the noise cached ON costs about the same as OFF within run noise (0.16-0.17 s vs 0.175 s). OFF has no extra work: the micro nodes are never built. ON reuses the generator's own mask node (shared, so the generator runs once per tile), and its extra work is one delta tile, a one-texel halo read and a whiteout per changed texel. The earlier ordered runs of the example showed the same (dirt on 0.13 s, wear on 0.11 s). Single machine, no quiet-machine controls: treat differences under 0.05 s as noise.
GPU compositor: sets with an active micro effect take the CPU graph path (the GPU declines them like spatial filters).
