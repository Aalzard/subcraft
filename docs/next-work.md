# Next work (triage)

Sources: GitHub issues #59 and #61, the Discord report, `docs/progress.md` "Next steps",
the roadmaps. (`docs/missing-features.md` and `docs/spp-research.md` do not exist in this
branch; the roadmap files were used instead.)

| # | Item | Impact | Effort | Risk | This session |
|---|------|--------|--------|------|--------------|
| 1 | #59 viewport texture wider than GPU limit crashes startup on big/multi-monitor setups (fix `fd894bd`) | high | hours | low | done (cherry-pick) |
| 2 | #61 no pen/tablet pressure at all | high | hours (days for Linux) | medium | pressure -> size/flow done where the OS gives pen force; Linux note below |
| 3 | Edge Wear still rejected on the baked default crate | high | days | high (needs bake data and visual check) | no (needs the owner's crate and eyes) |
| 4 | Brush wrapping across hard edges | medium | days | high (touches stamp resolve) | no |
| 5 | Filter / folder adjustment audits, effect drag reorder | medium | days | medium | no |
| 6 | 4K layered-asset performance profiling | medium | days | medium | no (needs a GPU session) |
| 7 | Cheap new filters (Gradient map, Histogram scan) | low-medium | hours | low | only if time is left |

## Pen on Linux / Wayland (#61)
SubCraft receives input through winit 0.30 via egui-winit. winit 0.30 reports pen force only
as touch events (Windows pointer/Ink, some touch screens). On Linux Wayland a Wacom tablet
is a tablet-v2 device; winit 0.30 does not expose `zwp_tablet_v2`, so the pen arrives as a
plain mouse with no pressure. Real support needs a newer winit or a direct tablet-protocol
dependency (new dependency: owner decision). Not done here.
