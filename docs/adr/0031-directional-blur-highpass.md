# ADR-0031 - Directional Blur and Highpass

Status: Accepted (2026-10-19). Extends ADR-0030; coordinate frame follows ADR-0005.

Extend the previous-stage DAG and existing commands/validation, rather than introduce flattened
filter images or another effect stack. Both filters work on supported layer content, masks and
isolated/passthrough adjustment folders. Normal vectors remain excluded. Source painting is
immutable; existing blend/opacity, ordering, visibility, duplication, history and sync export
contracts remain in effect. UI only edits parameters and dispatches app commands.

Directional Blur is a symmetric Gaussian line, radius 0-32 working pixels, sigma radius/3,
finite normalized support ceil(radius). Angle is -180..180 degrees and its axis repeats every
180 degrees. In top-left UV/image coordinates +v grows down: 0 is horizontal, 90 vertical,
45 down-right, -45 up-right. Cardinal axes are exact. Sample fractional offsets with bilinear
interpolation of PREMULTIPLIED color/alpha, accumulate weighted samples, then unpremultiply.
Coverage softens as for Blur; effect opacity mixes original/filtered premultiplied results.
Radius zero is exact identity under Normal effect blending.

A Directional node preloads the required bounded rectangular neighborhood once, with no cache
locks inside texel/tap loops. Sampling clamps at the global image boundary, never tile borders.
The existing sum of ceil(radius) supports bounds bilinear sample corners as well as chained
filters; it conservatively invalidates whole tiles. Reduced/enlarged previews scale radius only.
This remains a UV filter and can mix atlas islands; it does not follow geometry across UV seams.

Highpass reuses the separable Gaussian lowpass stage. Result = neutral + amount*(source-lowpass),
amount 0-4, neutral 0.5 for bounded color/scalar/mask inputs and 0 for signed Height. Preserve
original paint coverage. Non-Height results clamp to 0-1, including HDR emission; Height stays
signed. Work in linear color/raw data, without a new sRGB transform. Radius zero or amount zero
returns NEUTRAL, not the original image. Normal effect blending shows the detail field; Overlay
can combine it with original bounded color/masks, Signed Addition with Height.

A known uniform input's Gaussian/line lowpass is exactly its input: bypass convolution to avoid
roundoff residue in neutral Highpass outputs and unnecessary work. The same bounded, shared
stage cache, immutable worker snapshots and synchronous exact evaluator are retained.

Schema 11 adds directional_blur {radius,angle_deg} and highpass {radius,amount}. Versions 1-10
migrate by identity; an actual legacy Blur/Sharpen stack tagged schema 10 is pinned by a test.
Project and smart asset serialization/validation share these explicit parameters. Invalid
radii, angles, amounts and Normal targets reject before history changes and on load.

Numeric acceptance covers independent dense double-precision bilinear line sampling, image
and tile borders, cardinal/diagonal/fractional parameters, alpha/fringes, Highpass neutral/sign,
effect order/blends, folder modes, dirty halos and eviction. App checks undo, latest worker,
export/reopen and scaled previews; UI checks both domains. Native captures and CPU profiles are
recorded in progress.md. GPU processing, atlas-aware filtering and full 4K desktop/long-stroke
profiling remain separate milestones. The unresolved Edge Wear kernels are not changed.
