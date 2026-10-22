# ADR-0022 — Organic generator breakup and explicit pattern controls

Status: Accepted (2026-10-19)

Generator noise already stores frequency and seed but the editor did not expose them.
Periodic 2D triplanar fields can reuse the same pattern on opposite faces. New Edges/Cavity
generators start at 0.3 breakup and opt into a continuous object-space volume field. The
existing Edge Wear smart mask retains its authored 0.35 breakup.

The new field uses unwrapped 3D lattice hashing, quintic interpolation, four octaves and coarse
domain variation. It depends on normalized object position, not UV islands or projection axes.
Identical positions share the field across seams; its lack of a one-unit period reduces
face-to-face repetition. Reproducible integer seeds change the whole field. Existing periodic
patterns used elsewhere remain unchanged. The original tile field cache/budget is reused,
with an explicit distinct key namespace and exact scale/seed/remap bits.

Pattern size is shown as percent of the mesh's longest dimension (`100 / noise.scale`);
increasing it makes the breakup features larger. Frequency is stored in the existing field.
Seed is editable with a deterministic Next button (wrapping u32). These changes dispatch
normal app effect commands and participate in undo, save and smart-mask recipe round trips.

Project schema **7** writes `Generator.organic_breakup`. Its serde migration defaults absent
values in schemas 1–6 and older smart-mask/material recipes to false. Their noise amount,
frequency, seed and projected output remain unchanged, including an authored zero breakup.
New defaults apply only when constructing generators. Reduce Repetition explicitly opts an
existing generator into the volume field; switching it off restores projected behavior.

This decision does not implement distance-based edge/dirt spread or automatic layer-order
micro-normal dependencies. Those require a shared surface/stage contract (ADR-0016) and must
not be approximated by globally changing Levels or by reading future/own-layer normal output.
The owner request checklist is tracked in material-workflow-roadmap.md.
