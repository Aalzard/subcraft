# ADR-0032 - Contour Bevel and Mask Outline

Status: Accepted (2026-10-19). Extends ADR-0030/0031; coordinates follow ADR-0005.

Use the existing immutable previous-stage graph, effects, app commands and worker. Add a
Contour node, not a destructive raster rewrite or another effect stack. Content, masks and
folder adjustments share the evaluator. Normal vector channels remain excluded. UI edits
parameters and dispatches existing commands. Source, ordering, visibility, effect blending,
opacity, undo and synchronous export contracts remain unchanged.

Define a binary contour by thresholding the preceding stage: linear Rec.709 luminance for
color, raw first component for scalar/Height/masks, multiplied by original paint coverage.
Zero coverage is always outside. Threshold is finite -1..1 (UI offers negative values only
for signed Height). This is a shape operation producing grayscale; it does not retain hue.

Compute exact Euclidean distances between texel centers by two separable lower-envelope
passes over a bounded scalar rectangle (tile plus halo), O(rectangle area). Inside texels
measure distance to outside, outside to inside. Approximate the binary contour at nearest
opposite center minus half a pixel. This raster contour approximation is explicit: it is
not an analytic subpixel/vector contour or mesh geodesic. No imaginary opposite texels are
added outside the texture or at tile boundaries. Missing opposite texels have infinite
distance: uniform white bevels reach the plateau, uniform outlines are empty.

Bevel: radius 0..32 working UV pixels, Amount -1..1, Threshold. Inside result is
Amount * clamp(distance/radius,0,1), outside zero. Radius zero gives a hard plateau, not
identity. Negative Amount creates recessed signed Height; non-Height results clamp to 0..1.
The linear inward ramp is the initial Bevel profile. Bevel Smooth/custom profiles are later.
This is texture relief, not mesh beveling; use Height to drive the existing viewport bump.

Mask Outline: radius 0..32, Threshold, Position Inner/Outer/Both. On the chosen sides output
clamp(radius+0.5-distance,0,1), providing a one-pixel antialias transition; radius zero gives
zero. Both applies the radius to EACH side (twice the total nominal border width). Radius
and Threshold are independent. The source material is not recolored except when this filter
is applied directly to content. Use a mask for material borders.

Content filters preserve original alpha. Outward borders cannot reveal unpainted content;
put the outline on a fill layer's mask to reveal material beyond the source shape. Masks
are scalar and can expand into previously black regions. All normal effect blend/opacity
controls still apply. Signed Height remains signed, all other outputs bounded.

Contour halo conservatively uses ceil(radius+0.5) and adds through chained stages. Previews
scale radius by preview/source resolution. A uniform stage is transformed directly, without
a distance allocation. Temporary scalar distance arrays are bounded to one neighborhood;
there is no dense full-resolution RGBA image per layer. Completed stages retain the shared
bounded cache. Spatial CPU work remains off the UI thread; sync callers evaluate exactly.

Schema 12 adds bevel {radius,amount,threshold} and mask_outline {radius,threshold,position}.
Migration 1-11 is identity: no new stored fields are required on existing effects. A genuine
Directional Blur/Highpass stack tagged schema 11 must load unchanged. Project and smart
resource validation share the finite domains; stable JSON and complete stacks are tested.

Numeric acceptance compares distance passes to exhaustive nearest-feature search and tiled
results to independent brute-force contour goldens, including corners, fractional/zero
radii, content/masks/folders, signed uniform Height, alpha/opacity and chained dirty halos.
App tests cover scaled previews, latest worker edits, coalesced undo, exact export/reopen.
Native captures/profile details and their limits are recorded in progress.md. GPU backend,
atlas-aware boundaries and full desktop 4K/long-stroke profiling remain separate work.
Rejected geometric Edge Wear kernels are untouched; these UV filters do not fix that issue.
