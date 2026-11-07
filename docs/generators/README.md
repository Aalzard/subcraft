# Generator contact sheets

Rendered by `cargo run --release -p subcraft-app --example generator_sheet -- <prefix> <crate|cube|torus|sphere|slab> [size] [legacy]`
(CPU bake of AO and curvature, then the real compositor; ray-cast preview, no window).
Columns, left to right, top row: clean, dirt low, dirt mid, dirt high; bottom row: wear default, wear wide, wear fine, both.

- `before_*`: the previous model (`legacy`: no grain, old Spread defaults) on the crate and cube.
- `after_*`: new defaults (fine grain on Dirt and Edges, Spread 2.5, Variation 0.8) on the crate, cube and sphere.
- `*_uv.png`: the composited base color of texture set 0 in UV space, for seam checks.

Still weaker than Painter: no gravity streaks under ledges, no per-texel curvature smoothing at low bake sizes,
and the baked mesh maps are a CPU raycast with 32 AO samples (visible at 512).
