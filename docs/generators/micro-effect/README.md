# Generator micro effect

`micro_lit.png`, `micro_height.png`, `micro_normal.png` come from
`cargo run --release -p subcraft-app --example micro_sheet -- docs/generators/micro-effect/micro 2048 2.4`
(SubCraft_base with the default Dusty Plastic, plus a surface layer that holds a flat Normal and Height, plus a
Dirt layer and an Edges wear layer on top; real compositor, CPU bake, ray-cast preview). Columns, left to right:
Dirt, micro off; Dirt, micro on; Edge wear, micro off; Edge wear, micro on.

- `micro_lit.png`: base colour lit with the composited normal map. Wear on the right rim chips (carves) into a grainy
  relief; dirt on the base plate is raised and catches light at its rim.
- `micro_height.png`: composited Height (mid grey = 0, 5 grey levels per height unit). Dirt is lighter (raised), wear darker.
- `micro_normal.png`: the composited tangent-space normal map. Flat (0.5, 0.5, 1) outside the mask.

Iterations: v1 used the Surface layer's white base colour, which hid the plastic, and let Dirt's faint grunge tail
mottle the whole sphere (fixed: base colour off on the surface layer, soft knee on the mask); v2 showed sparkly specks on
dirt (fixed: normal gain 0.35 to 0.25, grain modulation 0.55..1 to 0.7..1). The images here are the final render.
Not shown: a surface layer with a real baked normal map (the shelf has none); the compositor test uses a tilted normal.
