# ADR-0045 — Anchor points

Status: Accepted (2026-11-09). Implements the anchor part of ADR-0016's planned stage contracts.

## Context

Substance Painter's Anchor Point effect captures a layer's content or mask stack at one point,
so other layers can read it: a mask effect, a fill's channel or a generator's input. Many
Painter smart materials depend on it (dirt or edge wear reading a lower layer's height or
normal detail), and `.spp` files contain anchors. SubCraft had none: the `.spp` importer baked
mask references into static rasters and skipped content anchors.

## Decision

**Data (doc).** An anchor is an effect: `ContentEffectKind::Anchor` or `MaskEffectKind::Anchor`.
Its name is the effect name, its identity the effect UUID (never the name). It changes nothing
where it sits. References hold an `AnchorInput { anchor: EffectId, channel }`:

* `FillSource::Anchor` in a fill layer's channel: the anchor's `(value, alpha)`;
* `MaskEffectKind::AnchorRef { input, levels }`: gray (color by Rec. 709 luminance) times
  coverage, through optional Levels (Painter's reference sources carry their own);
* `Generator::input`: replaces the baked map the generator reads (curvature for Edges and
  Concave, occlusion for Dirt and Ambient Occlusion, the map of Mesh Map kinds).

`channel` picks the channel of a content anchor; mask anchors hold one value and ignore it.

**What an anchor captures.** A content anchor: the layer's own content up to the anchor, per
channel, before its mask, blend and opacity (base paint or fill, folder children composited on
a transparent base, the content effects below it). A mask anchor: the mask up to it. Anchors
are captured even when their layer is hidden or masked out: a reference reads content, not the
layer's contribution. (Painter's "accumulate layers below" option is not implemented.)

**Order, cycles, missing anchors.** `doc::AnchorIndex` numbers one evaluation walk: per layer,
bottom to top, its folder children first, then its content base (where fill references sit),
its content effects, its mask effects. A reference resolves only to an anchor strictly earlier
in that walk, so dependencies are a DAG by construction. Anything else is reported, never
evaluated: `Missing` (no such id in the set, e.g. deleted or pasted from elsewhere),
`NotBelow`, or `Cycle` (the anchor's capture contains the reference, e.g. a folder anchor read
by its own child). An unresolved reference evaluates as if absent (a fill channel is
transparent, an Anchor Reference adds nothing, a generator falls back to its baked map). The UI
shows the reason on the reference and a warning on the layer row.

**Evaluation (compositor).** References are evaluated by the stage graph (ADR-0029/0030):
`needs_graph = has_spatial || has_references`. With references present the graph indexes the
anchors and builds, on demand and once per (layer, channel), the layer's isolated content chain
and its mask chain, recording the node after every effect; an anchor is the node at its index,
a reference an input edge to it (`Node::AnchorRead`, the `anchor` input of `Node::MaskEffect`).
The layer's own blend reuses the same chain, so an anchored stack is evaluated once per tile.
Without references the graph and the per-tile evaluator are exactly as before: no anchor code
runs, anchors alone are identity effects skipped by both evaluators.

**GPU path (ADR-0044).** A set with references still uses the GPU: masks and fill channels that
read anchors are evaluated on the CPU through a stage graph built for the batch (anchor captures
included) and uploaded as blocks; the GPU blends them like any other CPU-evaluated source.
Results match the CPU within ADR-0044's tolerance (test `anchored_sets_run_on_the_gpu_and_match_the_cpu`).

**Invalidation.** References read texel by texel, so a raster edit of an anchored layer changes
its readers at the same texels. When a set has references, the incremental diff grows raster
edits by the halo of every filter in the set (conservative: a filter on a reader's path spreads
the change). Parameter edits already re-evaluate everything.

**Copies.** Duplicating, copy/pasting several layers, pasting masks and mask effects remap the
references inside the copy to the copied anchors; references to anchors outside the copy keep
pointing at the original.

**Persistence.** Project schema 17: `anchor` content / mask effects, `anchor_ref` mask effects,
the `anchor` fill source and the generator `input` (omitted when absent). Schemas 1-16 load
unchanged; older SubCraft refuses schema 17 instead of dropping anchors. Smart materials carry
them through the layer schema.

**`.spp` import.** Painter anchors in content and mask stacks become anchor points (named after
the Painter label, or "<layer> anchor"); references in masks become Anchor References with their
Levels, references in fill channels become anchor fill sources. Links are live. A reference whose
anchor was not rebuilt stays in place, flagged as missing (counted in the import report).

## Consequences

* Isolated thumbnails (`preview_content`, mask previews) are evaluated without the texture set,
  so they show references as unresolved; the composite and the viewport are exact.
* Fill content effects cannot read anchors yet (fill layers can).
* Cross-texture-set anchors and "accumulate layers below" need a deliberate extension of the
  ordering rule.
