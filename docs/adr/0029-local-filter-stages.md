# ADR-0029 — Local filter effects and folder adjustment stages

Status: Accepted (2026-10-19).

[Adobe's filter documentation](https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/effects/filters/filter)
describes editable transforms on separate content/mask stacks and passthrough processing of
lower layers. Reuse SubCraft's UUID effects, snapshots, commands, thumbnail evaluator and
immutable composite worker. Extend the existing stages rather than add flattened filter images.

`doc::Filter` stores Contrast, HSL, Threshold and Clamp parameters. These are local operations:
input = previous stage at the same texel, dependency halo = 0. Deterministic CPU kernels live
in materials; app/project validate parameters and channel domains. Normal vectors cannot be
processed by these filters. HSL is restricted to color channels; masks expose the other three.
Contrast is component-wise around a pivot, preserving signed Height; Threshold uses linear
Rec.709 luminance for color, raw values for scalars; Clamp does not rescale its input. HSL
converts bounded color to encoded sRGB, operates on actual HSL (not HSV), then returns linear
RGB. Nonidentity HSL is display-referred and clamps HDR input; identity is an exact bypass.

Content adjustments preserve original coverage, with selected-channel blend/opacity; they run
before the layer's mask and blend. Mask filters transform the running grayscale mask using the
existing effect blend/opacity. Sources stay immutable. Parameter edits dirty tiles through
existing parameter equality; raster edits need no halo expansion for these local kernels.
Thumbnails, save/export and worker output use the same evaluator.

Folders now accept adjustment effects only (Levels/Filter); Paint/Fill sources remain rejected.
An isolated folder filters its children's composite before its own mask/blend/opacity. A
passthrough folder filters its child-plus-lower-stack accumulation, then mixes that result
using the folder's existing mask and per-channel opacity. An empty passthrough folder is an
adjustment group. Normal paint/fill layers retain isolated content semantics.

Schema 9 adds explicit Filter variants to both effect schemas and allows folder adjustments.
Schemas 1–8 migrate by identity: absent filter variants/stacks retain their original data.
Smart masks/materials reuse the same schema and validation. Older readers reject schema 9
instead of silently losing effects. Invalid domains, nonfinite parameters and reversed Clamp
bounds are rejected before history changes and on load.

This does not supply neighborhood stages, blur/sharpen/bevel/warp, a GPU filter backend,
references/anchors or filter shelf resources. Those need ADR-0016's explicit graph/halos;
never approximate a multi-tile blur by filtering each 64px tile independently.
