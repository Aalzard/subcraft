# ADR-0030 - Neighborhood filter stages

Status: Accepted (2026-10-19). Extends ADR-0029.

Blur and Sharpen read the immediately preceding content or mask stage, including neighboring
UV texels. A filter must never clamp sampling at an internal 64px tile boundary or read a
partially updated final composite. Keep the fast existing evaluator for stacks without spatial
filters. Otherwise compositor/stages.rs builds an immutable, channel-specific dependency DAG;
all dependencies have lower node IDs. Paint/fill sources, local adjustments, masks and folder
blends retain their existing ordering. Point-stage kernels and generator evaluation are shared
with the old evaluator. This does not modify the unresolved Edge Wear kernels.

Gaussian blur uses separable finite-support weights, support ceil(radius), sigma radius/3,
normalized in materials/filter.rs. Radius is 0-32 working texture pixels; fractional values are
valid, zero is identity. Global image edges clamp. Filtering is in UV texture space: it does
not follow mesh adjacency, bridge geometric UV seams or prevent mixing nearby atlas islands.
Changing working resolution changes apparent radius. App previews pass a resolution ratio to
the same stage evaluator, including enlarged previews, without changing document parameters.

Content Blur filters premultiplied linear RGB and coverage, then unpremultiplies. Thus it can
expand painted coverage without dark fringes. Filter opacity mixes original and filtered
premultiplied results; it does not source-over the blurred image, which would increase coverage
even for identity. The chosen value blend applies before this mixture. Sharpen is unsharp mask
(source + amount*(source-blurred)); amount 0-4, original coverage retained. Signed Height is
preserved; other sharpen values are bounded to 0-1. Mask filters use the running scalar field
and existing mask blend/opacity. Normal vectors remain unsupported. No implicit sRGB conversion
is performed by either kernel; values remain in their channel's working domain.

A per-set StageCache retains at most 1024 completed RGBA tiles (64 MiB), plus temporary active
dependency frontiers. OnceLock cells share computation across parallel requests; only completed,
unborrowed cells can be evicted. Trim again after a batch. Tile-independent stages (uniform sources and dependent adjustments/unmasked blends) share one
result across tiles. Empty paint channels and disabled fill channels with no effects are skipped,
matching the existing evaluator even under Replace blending. This avoids filling the cache with
identical default-channel tiles and preserves absent-source semantics.
Resolve long primary same-tile chains iteratively; neighborhood dependencies point strictly
backward, preventing cycles. Eviction changes cost, never values. There is no dense image per
stage. Previews use short-lived bounded caches. A changed snapshot or external invalidation
clears cached stages; unchanged budgeted batches reuse them. Node IDs are plan-local, not saved.

Raster dirtiness expands conservatively by the sum of active neighborhood supports, rounded
out to tiles. This deliberately over-invalidates independent branches; parameter/structural
changes still invalidate all. Pending work always evaluates the latest snapshot. The existing
worker owns viewport computation; synchronous export/save/tests drain the same exact evaluator.
No GPU processing is introduced here. GPU parity and complete desktop frame profiling remain
separate milestones; CPU polling timings alone cannot establish smooth 4K rendering.

Schema 10 adds the Blur/Sharpen parameter variants. Versions 1-9 migrate by identity, retaining
all existing stacks; explicit schema-9 compatibility is tested. Project and smart resources
share parameter validation and persistence. UI only dispatches existing effect commands.
