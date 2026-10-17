# Handoff to Claude — 2026-10-18

- Branch: `feature/brush-roadmap`, from `origin/main` `cef93c9`; nothing pushed/released.
- Last code commit: `90c676532859394840f5ad1e96cc5f509b2b00a7`; this file is in the final
  documentation commit (`git rev-parse HEAD`).
- `cargo test --workspace`: **374 passed, 0 failed, 0 ignored**. Clippy
  `--workspace --all-targets`: exit 0, existing warnings only. Native Windows/RTX 4060 D3D12 ran.

Session 62 — cleanup requested; partial:
- Latest preview copied to `target/release/subcraft.exe`; SHA256
  `0BBDF48783CE6E38F88DD25EA7B65EBF81D76C5D83F17A216696AF6A09479959` matches.
  Native demo/screenshot smoke test exits 0. No source/visual changes. Tests above rerun.
- Deletion stops at the shell tool: automatic approval review rejected bulk cleanup and the
  safer explicit target/debug deletion, saying only “blocked by policy”. Neither ran. No space
  reclaimed; ~22.2 GB of build artifacts and old dist packages still present. Do not bypass
  this restriction or claim cleanup complete. Protected files and user deletions untouched.

Session 61 — panel contrast complete:
- Strengthened initially too-faint panel bands to 10–13 RGB levels apart in theme.rs; section
  headers are clearer too. Shared widgets keep hover brighter than both shades and orange
  selection dominant. Added actual pointer/color regression; no app/document/paint change.
- Native CLI screenshot verified Parts/History/Layers and Brush headers:
  `docs/screenshots/panel-row-contrast.png`. Current test build: `target/release/subcraft-preview.exe`.
  Standard and preview now contain this same latest build (consolidated in session 62).

Prior UI priorities (complete in sessions 54–60):
- Compact Brush/Eraser/Fill icons, unboxed Size/Opacity, Flow/Hardness slider popovers; full
  Anti-aliasing text and wider default canvas. Code `92de657`.
- Subtle Parts/History/Layers bands, more spacing, banded section/property rows and icons;
  calmer text/actions, almost-square 2px corners and Material header overlap fix. `a7393e4`, `29f3042`.
- Cached, depth-tested/faded floor grid and red X / green Y / blue Z; Y off by default;
  toolbar toggle/right-click axis settings saved in viewport prefs. `b3a19b1`.
- Toolbar mirror popover owns X/Y/Z + Lazy Mouse; sidebar/rail duplicates removed;
  shared app SetStrokeOptions command and existing presets. `d143a94`.
- UI size first in File / Edit → Preferences: live 75–150%, default/reset 95%, persisted;
  exact rounded viewport mapping fixes fractional-scale paint drift. `5f9b995`.

Stops/limits:
- No partial work remains in the requested UI list. The larger brush roadmap is still partial.
  Phase 1 stops at `paint/src/stroke.rs::Stroke::write` (static source sampled after coverage);
  camera/per-stamp need stamp-dependent accumulation. `paint/src/target.rs::TargetImage::compute_tile`
  owns source snapshot/normal blending. Projection-frame/mirrored-tangent normals and async
  starter-pack brush destinations (`app/src/asset_apply.rs::Destination`) remain.
- Session 61 used native screenshot mode only: no hand hover/selection check, other displays/DPI
  or platforms. Synthetic egui pointer tests cover hover, selection and child-control routing.
  Historical screenshot deletions present at session start remain unstaged; do not restore or
  commit them without owner direction. Protected untracked files remain untouched.
- Native tool switching/Flow/Hardness/typed Size were checked before the owner stopped Computer
  Use with Escape. No further Computer Use calls were made; later checks used native CLI
  screenshots. New grid/symmetry/scale gestures, tablet, mixed-DPI and other platforms are not
  hand-verified. Real egui input tests, GPU grid acceptance and paint texels at 75/95/150% pass.
- No failing/ignored tests remain. One initial async shelf-import polling test flaked under a
  concurrent build; full reruns passed without weakening it. Full 4K frame pacing is unmeasured;
  prior CPU 4K stroke-to p95 1.68ms/max 3.72ms is not whole-frame certification.
- Owner app windows/data were left untouched. Standard and preview now match.

Keep: dependency direction; tiled COW storage; one undo per stroke; immutable source snapshots;
shared RNM; one app brush settings/options store. Grid/UI size are display preferences only.
Project schema stays 4, brush preset version 2. See ADRs 0011–0015. Owner question for Phase 9:
future X color-swap conflicts with the existing X mirror shortcut; resolve when editing shortcuts.

Next 3 steps:
1. Resolve the environment deletion restriction before completing artifact cleanup; preserve
   the standard executable/hash above. Then resume pending native UI gesture checks.
2. Implement camera/per-stamp sources in Stroke with batching/symmetry/alpha/undo numeric tests.
3. Finish projection/mirrored-tangent normal reorientation, then follow the ordered roadmap for
   async brush drops, complex-model checks and full 4K paint→composite→upload profiling.
