# ADR-0015 — One saved interface scale, applied to the whole egui context

Status: Accepted (2026-10-18).

UI size is the first setting in File / Edit → Preferences. Its saved percentage controls egui's
context zoom, scaling fonts, icons, widgets and layout together while preserving the native
display DPI multiplier. Default 95% provides the owner's slightly smaller interface; 75–150%
and Reset → 95% allow adjustment without restarting. The Preferences body scrolls at large
scales/small windows. The existing eframe Preferences store remains the sole source of truth.
Old settings receive 95% via serde defaults; invalid sizes clamp before applying. This is
display-only data, so document schema and brush presets do not change.

Fractional UI zoom produces fractional scene rectangles but whole-pixel render targets. UI
input maps normalized scene coordinates through the exact rounded camera viewport dimensions,
not only pixels-per-point. Pen/stencil inverse projection and cursor/pan conversion use the
same dimensions. This fixes subpixel paint drift exposed by scale acceptance tests; camera,
mesh, UV, brush-size and undo conventions remain those in ADR-0005.

Tests cover legacy preference loading, bounds and serialization; actual slider/reset input,
context zoom and first-setting order; File-menu access; unchanged document/tool state; and
premultiplied paint texels at 75/95/150% (max component difference ≤0.005, one undo step per dab).
Existing pen/hover and numeric paint regressions remain in the workspace acceptance suite.
