# ADR-0034 - Effect drag reordering

Status: Accepted (2026-10-20). Extends ADR-0017/0033; project schema remains 12.

A drop reorders effects within their existing layer and content/mask domain. App owns the
address and index rules: EffectAddress contains texture-set, layer, domain and effect UUIDs;
DropSide is visually Above/Below. app::effect_stack::drop_destination validates both live
addresses, finds their current positions and resolves a bottom-to-top index AFTER removal.
Stale identities, cross-layer/domain/set addresses, self-drops and unchanged positions return
None. No document/history mutation occurs while hovering or on an unchanged/invalid target.
The controller rejects a drag whose originating texture set is no longer active.

On release, DropEffect revalidates the UUID anchors, invokes the existing MoveContentEffect
or MoveMaskEffect command exactly once, then selects the moved effect using SelectEffect.
The non-coalesced move creates one undo entry; selection adds none. Original effect/raster
Arcs, IDs, settings, blend and visibility survive. Existing snapshot diffing updates composites,
worker jobs and stage thumbnails. Save/export stay synchronous and exact. Order already lives
in the versioned schema, so this interaction requires no new document fields or migration.

UI maps pointer position above/below a row center into DropSide. Rows and preview squares can
start a drag; names are non-selectable text so text selection cannot steal the gesture. A thin
amber insertion line is shown only for a valid changed destination. UI passes the command to
app on release without directly reordering vectors. Typed payload guards preserve existing
layer/shelf drag handling. The existing scroll-area edge behavior accepts effect payloads.
Escape uses egui's drag cancellation; release outside a valid row makes no document change.
Context-menu Move up/down remains available. There is no cross-layer effect transfer in this slice.

Pure controller tests exhaust source/anchor/side pairs in both domains. Numeric app acceptance
covers non-commuting filter order over all 128px texels, inclusive stage previews, one undo,
redo, sync export/reopen and raster identity/brush routing. Headless egui tests use real pointer
press/move/release events for names and squares, both directions/domains, insertion markers,
Escape, outside/no-op drops, foreign payloads and edge scrolling. Native captures show the
post-drop order and Properties on the actual default crate; they do not establish manual
native dragging. CPU drop/preview timing and verification limits are recorded in progress.md.
