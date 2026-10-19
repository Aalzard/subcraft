# ADR-0019 — High-poly bake sources and visible normal results

Status: Accepted (2026-10-19)

## Problem

The shared mesh importer required UVs even for the high-poly ray-cast source. A sculpt without
UVs failed before baking. Errors were only transient status text. Successful normal maps were
stored as mesh maps but did not affect the material unless the owner manually connected them.
Total projection misses silently produced flat low-poly normals; partial misses could use a
low-poly triangle index to access high-poly element IDs and panic the worker.

## Decision

Keep paint-target import strict. `MeshBuilder::build_for_baking` and
`import_mesh_for_baking` accept high-poly geometry without UVs, retaining canonical positions,
normals, material/object identity and valid attribute arrays. Zero UVs are placeholders, not an
unwrap; the baker uses the low-poly UV atlas and tangent frame. All supported import formats
share this path. Models retain their exported transforms; do not independently normalize them.

The baker represents a projection miss explicitly. Zero high-poly hits return an actionable
alignment/cage error. Partial misses use low surface normals/positions and neutral IDs, never
indexing the high mesh with low triangle IDs. Existing AO/curvature sampling behavior remains.

After a successful background high-poly Normal bake, `app` adds one normal-only fill layer at
the top of the affected texture set: **Baked normal · high poly**. Its live `MeshMap::Normal`
source stays in UV space, and Inverse Detail RNM treats the baked orientation as the base with
authored normals underneath as detail. Other material channels are untouched. Layer creation
is undoable; baked mesh maps retain the existing separately saved, non-history storage.

An existing enabled Normal mesh-map reference in a fill or content fill effect (including a
folder descendant) prevents insertion, preserving authored workflows and avoiding double
application on rebake. Hidden/masked references are still intentional owner-authored links;
rebaking does not override their visibility. The new layer can be hidden, moved or removed.
New layers placed above it follow ordinary channel blend rules. No implicit compositor or
export fallback is added. Viewport, export and save/reopen therefore use the same layer result.

Bake completion/errors remain in the Bake dialog until the next run. Partial errors are
reported with the number of completed sets; unexpected worker disconnects report failure.
UI displays this state and dispatches requests; mesh and app logic stay outside widgets.

The existing versioned layer/source schema already represents this data; no schema migration
or new project fields are needed. Low-on-low baking and direct `set_mesh_maps` callers retain
their current behavior. `--bake-high path --bake-size size` starts the same application job for
repeatable native checks; screenshots wait for the job and composite to finish.

## Validation and limits

Numeric tests cover UV-less import versus strict low import, known sloped high normals,
fully missed cages, partial-miss IDs, visible composite/export values, live rebakes, layer
undo/redo, exact save/reopen and partial failures. Headless UI tests exercise the actual Bake
button and persistent error text. Native screenshots and manual verification limits are in
progress.md. Mesh-name matching, custom cages and automatic alignment remain future work.
