# ADR-0047 — Procedural materials: exposed parameters bound to layers

Status: Accepted (2026-11-23)

## Context

The shelf's materials are either bitmap packs or fixed recipes. A smart material is a folder of
fills masked by generators, built once by code: to change the paint colour or the amount of wear
the user opens the folder, finds the right layer or generator and edits it there. Substance
Painter's materials are Substance graphs with a short list of sliders (colour, wear, dirt, scale)
that stay editable after the material is applied. `.sbsar` cannot be read (the engine is closed),
so the gap report asked for SubCraft's own parametric format.

Two designs were considered.

**A node graph evaluated per texel** (a new fill source: noise, math, ramp and mesh-map nodes with
one output per channel). It is the most expressive, and the most expensive: a second evaluator
next to the layer compositor, with its own caching, its own GPU story (ADR-0044 blends on the GPU
but evaluates sources on the CPU), its own editor, and none of the generators the owner tuned
(edge wear, dirt, micro details live in the compositor, above `materials` in the dependency
order). Months of work before the first good-looking material.

**Parameters bound to a layer stack.** A layer stack already is a graph: fills are the sources,
masks and generators the selectors, blend modes and content effects (levels, gradient map, HSL,
blur, the MatFinish filters the filters thread is adding) the operators. What is missing is only
the short list of sliders on top.

## Decision

**A procedural material is an ordinary layer that also carries `MaterialControls`**
(`subcraft_doc::material`): a fill layer, or a folder of masked fills, plus

* **parameters**: `id`, label, group, hover text, a kind (`float` with a range, `color`, `toggle`,
  `seed`), a default and the current value;
* **bindings**: "property P of layer L (or of mask / content effect E of layer L) is formula F of
  the parameters".

Setting a parameter evaluates every binding and **writes the results into the layers**. The
compositor, the GPU path, export, thumbnails, instances, the MCP server and every tool keep seeing
plain layers: there is no second evaluator and nothing new to cache. The user can still open the
folder and edit any layer by hand; a binding whose target was deleted or changed type is idle
(reported by `MaterialControls::check`, never an error).

**Formulas** (`subcraft_doc::material_expr`) are plain text: numbers, parameter ids, `+ - * /`,
parentheses and a fixed list of functions (`mix clamp min max pow abs saturate step smoothstep rgb
luma select remap round`). Every value has three components, a number being the same in all three,
so colours and numbers mix (`paint_color * 0.5`, `clamp(roughness - variation * 0.5, 0, 1)`).
Formulas are bounded in length and nesting, division by zero is 0, and a formula that does not
parse or names an unknown parameter makes its binding idle. They are text in the file on purpose:
readable, diffable, writable by hand.

**Bindable properties** (`Prop`): layer visibility and opacity (per channel or all); a uniform
channel's value; a procedural channel's low / high / scale / balance / contrast / seed; channel
strength; projection tiling and rotation; and on a mask effect its visibility and opacity, a
generator's level, softness, breakup amount / scale / seed, grain, grunge floor, edges masking, AO
masking, smoothness and spread, and a mask fill's value and pattern settings. Every write is
clamped to what the editor itself accepts. New properties are new enum variants.

**Identity.** Bindings name their targets by layer and effect UUID, never by position or name.
`Layer::duplicate_with_new_ids` (duplicate, paste, applying a saved smart material) rewrites the
ids, so every copy drives its own layers. An instance (ADR for instances) shows what its source's
parameters produce and carries none itself.

**Commands.** `SetMaterialParam` (coalesces per parameter while a slider is dragged, like
opacity), `ResetMaterialParams`, `SetMaterialControls` (attach / replace / remove; the definition
is sanitized). Properties shows a "Material parameters" section for any layer that carries
controls, with the app's usual reset gestures.

**File format.** `LayerV1.material` holds `{ "params": [...], "bindings": [...] }`: the JSON of
`Param` and `Binding`, pinned by a test. It is additive and optional, so the schema version is not
bumped: an older SubCraft ignores it and sees the layers as the parameters last left them. Entries
are read one by one; one that a newer version wrote (an unknown property) is skipped instead of
failing the project. Because user smart materials are saved as layer fragments, **a procedural
material file is a smart-material file whose root layer has `material`**: saving a tweaked or
hand-made material to the user library keeps its sliders, with no new file type.

**Built-ins** (`subcraft_assets::procedural`): eleven materials (Painted Metal, Rusted Steel,
Brushed / Polished / Hammered Metal, Aged Copper, Worn Plastic, Rubber, Concrete, Leather, Wood) in
the shelf's Smart materials under the "Procedural" category. Each is built at its defaults, ends
with Scale and Seed, and keeps the tuned Dirt / Edge wear generators exactly at their defaults
when its own parameters are at theirs.

## Consequences

* Sliders after applying, saved in projects, for the cost of one small pure module and one
  Properties section. No performance path changes: a slider costs what editing the same
  properties by hand costs.
* A material can only do what layers can do. Richer base surfaces come from new patterns, filters
  and generators, which every layer then benefits from, not from a private node set. If a real
  per-texel graph is ever needed it can be one more fill source and still be driven by these same
  parameters.
* A property the user edits by hand is overwritten the next time a parameter that drives it moves.
  This is the same rule as Painter's (the instance parameters own what they drive) and is shown
  nowhere yet: a marker on bound rows is future work.
* Authoring (added after the first version): "Add parameter" in the Material parameters section
  lists every property of the layer tree that can be exposed (`material::exposable`) and adds a
  parameter whose default is the property's value now, bound with the formula `<id>`. The gear
  toggles an edit mode to rename, change a slider's range, remove, or make the current values the
  defaults. Each step sends the whole definition as one `SetMaterialControls`.
  "Also drive..." links one more property to a parameter and every binding's formula is editable
  there.
* Not built yet: choice
  parameters (dropdowns), bindings on content-effect settings (filters), translated labels for
  built-in parameters, parameters on brush materials.
