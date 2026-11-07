# Generator contact sheets

Rendered by `cargo run --release -p subcraft-app --example generator_sheet -- <prefix> <crate|cube|torus|sphere|slab> [size] [legacy]`
(CPU bake of AO and curvature, then the real compositor; ray-cast preview, no window).
Columns, left to right, top row: clean, dirt low, dirt mid, dirt high; bottom row: wear default, wear wide, wear fine, both.

- `before_*`: the previous model (`legacy`: no grain, old Spread defaults) on the crate and cube.
- `after_*`: new defaults (fine grain on Dirt and Edges, Spread 2.5, Variation 0.8) on the crate, cube and sphere.
- `*_uv.png`: the composited base color of texture set 0 in UV space, for seam checks.

Still weaker than Painter: no gravity streaks under ledges, no per-texel curvature smoothing at low bake sizes,
and the baked mesh maps are a CPU raycast with 32 AO samples (visible at 512).

## Timings (crate, 2 texture sets, 3 layers, 16 threads, machine shared with other builds at ~100% CPU)

Full recomposite after changing the two masked layers, release build, summed thread time in brackets:

| size | clean (cold) | with dirt/wear active |
|---|---|---|
| 1024 | 0.2-0.4 s | 0.4-0.7 s (generators ~1 s of thread time) |
| 1536 | 1.9 s | 4.9 s |
| 2048 | 5.4-6.0 s | 12-16 s (generators 25-40 s thread time of ~210 s) |

Findings: generators are about 15% of evaluation time at 2048; the rest is the per-tile stack (mask read, fill evaluation, blend)
which does not scale linearly with texels (4x texels from 1024 to 2048 costs ~16-30x). The numbers were taken while other
sessions kept the CPU at ~100%, so absolute values are inflated and the cause of the superlinear growth is not confirmed. No
performance fix was made.
