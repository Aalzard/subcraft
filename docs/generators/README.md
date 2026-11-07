# Generator contact sheets

Rendered by `cargo run --release -p subcraft-app --example generator_sheet -- <prefix> <crate|cube|torus|sphere|slab> [size] [legacy]`
(CPU bake of AO and curvature, then the real compositor; ray-cast preview, no window).
Columns, left to right, top row: clean, dirt low, dirt mid, dirt high; bottom row: wear default, wear wide, wear fine, both.

- `before_*`: the previous model (`legacy`: no grain, old Spread defaults) on the crate and cube.
- `after_*`: new defaults (fine grain on Dirt and Edges, Spread 2.5, Variation 0.8) on the crate, cube and sphere.
- `*_uv.png`: the composited base color of texture set 0 in UV space, for seam checks.

Still weaker than Painter: no gravity streaks under ledges, no per-texel curvature smoothing at low bake sizes,
and the baked mesh maps are a CPU raycast with 32 AO samples (visible at 512).

## Timings (crate, 2 texture sets, 3 layers, 16 threads, release)

Full recomposite after changing the two masked layers. The earlier 12-16 s at 2048 came from the example harness
using the Windows system heap: per-tile buffers (64 KB and up) are allocated on every core and the heap serializes
them, so 16 threads were slower than one. The app itself uses mimalloc (`apps/subcraft/src/main.rs`); the example now
does too. Same machine, same code, only the allocator differs:

| size | system heap, clean / with generators | mimalloc, clean / with generators |
|---|---|---|
| 1024 | 0.2-0.4 s / 0.4-0.7 s | 0.05 s / 0.03-0.13 s |
| 2048 | 5.4-6.0 s / 12-16 s | 0.20 s / 0.13-0.52 s |
| 2048 bake (AO + curvature, CPU) | 10-35 s | 5.4 s |

Single-threaded cost is linear in texels (512: 0.23 s, 1024: 1.0 s, 1536: 2.6 s clean), so there is no algorithmic cliff;
the first evaluation after changing noise parameters is the slowest (fields are recomputed), later ones reuse them.
Any other harness that runs the compositor or importer without mimalloc (for example `spp_lab`) pays the same penalty.
