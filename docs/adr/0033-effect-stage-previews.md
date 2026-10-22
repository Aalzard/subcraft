# ADR-0033 - Effect stage thumbnails

Status: Accepted (2026-10-20). Extends ADR-0016, ADR-0017 and neighborhood stages ADR-0030-0032.

Each effect thumbnail shows its inclusive stage: the authored layer/mask base plus all effects
through that effect in evaluation order. Later effects cannot influence the result. This is
an evaluated stage preview, not a source-resource icon or the final layer repeated on every row.
Disabled effects remain bypassed; their thumbnail therefore shows the preceding result, with
a diagonal UI mark. A disabled parent layer/mask still permits authored-content inspection.
Content ignores its own layer mask, parts, visibility and opacity, as existing layer previews do.

Extend the transient app PreviewKey with Option<EffectId>, retaining layer UUID, domain, channel
and size. The worker truncates only a private COW snapshot; document data/history never change.
Paint/Fill effects use the Layers active channel. Levels/Filter effects preview their explicit
target channel even when Layers displays another channel. Masks are always raw grayscale;
color content is encoded linear-to-sRGB for display, data channels remain raw. Alpha is shown
against the existing checker. Height preview continues to clamp to display range, not remap signs.

Folders require their actual stage input. App captures immutable root-layer references for
folder content previews and invalidates on changed root pointers. Compositor follows the
ancestor path: passthrough inherits accumulated lower input, isolated ancestors start transparent.
Ancestor effects/masks and later siblings run after the target and are excluded. The target
folder retains passthrough/isolated semantics, with its own presentation mask/opacity bypassed.
This also corrects empty passthrough folder thumbnails, previously evaluated against transparency.
Non-folder content previews remain independent of other layers. UI never assembles effect stages.

Reuse the existing single background preview job, immutable resource/map/geometry snapshots,
stale-result rejection, latest queued snapshot and 96-entry CPU/GPU LRU bounds. Validate the
UUID in the requested domain before queuing; removed effects return no preview and remove their
queued/cache entries. Only visible squares request work. Previews are 64px (inspection remains
bounded at 256px), not full 4K readbacks; spatial radii scale from working resolution. Fine marks
can disappear at this reduced sampling. Different prefixes currently evaluate independently;
sharing completed prefix nodes is a later optimization if measurement justifies it.

UI owns upload, checker, selection border and disabled mark. Effect squares are 18-28 UI points,
controlled proportionally by the existing Layers Preview size preference; row height follows.
Retain the small type icon and name. Clicking the square always selects that effect through
SelectEffect, without toggling back or adding history. Existing text-row toggle/context menus,
independent content/mask targets, scrolling and collapse behavior remain intact.

No document field or project migration is added: project schema stays 12. Previews are regenerated
after reopen rather than saved into files. No new shortcut, GPU readback or renderer dependency
is introduced in app/compositor. Numeric tests cover inclusive stage values, visibility, channel
encoding, alpha, spatial radius scaling, folder scope and lower-input invalidation. App tests
cover latest edit/undo/reorder/removal/reopen, cache bounds and a 4K Paint-effect stroke. UI tests
cover compact hit targets, selection without history, sizing and collapse. Native captures and
CPU request/stroke timing are recorded in progress.md; full GPU/frame pacing remains separate.
