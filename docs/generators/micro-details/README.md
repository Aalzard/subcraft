# Generator micro details (Painter's "Use Micro Details")

`micro_details_lit.png` and `micro_details_mask.png` come from
`cargo run --release -p subcraft-app --example micro_details_sheet -- docs/generators/micro-details/micro_details 2048 2.4`
(SubCraft_base, CPU bake of curvature + AO, real compositor, ray-cast preview).

The model gets a surface layer with a tri-planar procedural **Height** only: raised plates with thin grooves between
them. None of that relief is in the baked maps. On top sit a default Dirt layer and a default Edge wear layer.
Columns, left to right: Dirt; Dirt + micro details; Edge wear; Edge wear + micro details (Amount 1, everything else
at its default).

- Without micro details both generators only know the baked mesh: dirt is grunge over the baked occlusion and the wear
  sits on the rim of the hole only.
- With micro details the generators also read the Normal / Height of the layers below: dirt settles into the grooves
  of the material, and wear chips the lips of the grooves, broken up by the same grunge as the mesh edges.
- `micro_details_mask.png` is the generator mask alone (white = covered).

Note: the preview shades with the composited Normal only, so the height relief itself is faint in the lit image; in the
app the viewport shows Height as bump.

Timings at 2048 (4 cores, two texture sets, full recomposite): micro details off 0.36 to 0.42 s, on 0.30 to 0.34 s
(the on case runs on the stage graph, which here happens to be no slower).
