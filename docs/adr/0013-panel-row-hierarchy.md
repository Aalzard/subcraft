# ADR-0013 — Quiet panel row hierarchy

Status: Accepted (2026-10-18).

Parts, History and Layers use full-width 34px rows alternating between two charcoal colors
with RGB (22, 23, 26) / (32, 34, 39), 10–13 levels apart. The original 5–7-level
separation proved too faint in actual use; this follow-up keeps the charcoal palette but makes
the alternation legible. Hover brightens either band to (38, 40, 46), and the existing orange
selection takes precedence. Section headers alternate (24, 26, 30) / (34, 36, 41). Indented
layer children stripe in visible order; folder/visibility/opacity controls retain input priority.
Parts uses the same row geometry, with a cube/hidden icon and its existing context-menu actions.

Brush/Properties reuse the shared section/property widgets. Sections have 32px banded headers,
disclosure arrows and existing Phosphor glyphs where meaningful; property rows have a minimum
28px height and quieter alternating backgrounds. Independent section/property counters live
only in egui temporary data, keyed by the containing panel's unique UI ID and render pass.
Nested horizontal/disabled scopes discover their containing panel through the UI stack. Every
pass starts from zero; no new child scope changes saved widget IDs or expansion state.

This is presentation state in ui only. No document, undo, preset or project-schema change is
introduced. Tests inspect actual egui geometry/colors/input routing and existing layer drags.
Native screenshot evidence and explicit hand-verification limits are recorded in progress.md.

The follow-up material-panel refinement lowers body text to a softer gray, gives interactive
controls a 2px almost-square radius (the owner's final preference), and uses muted, borderless
secondary image actions with 28px hit targets.
Channel cards separate their header/image action from flat-value editors. Header actions render
in an unallocated child UI: the parent already allocated the header rectangle, so a second
cursor advance must not pull the section body over its header. Regression tests check the
actual Material heading/button bounds at 280–420px sidebar widths.
