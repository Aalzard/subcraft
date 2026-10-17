# Handoff to Claude — 2026-10-18

- Branch: `feature/brush-roadmap`, from `origin/main` `cef93c9`; nothing pushed/released.
- Last code commit: `5f9b9956590cf887f4269684d4b1e6fffd67e44d`; this file is in the final
  documentation commit (`git rev-parse HEAD`).
- `cargo test --workspace`: **373 passed, 0 failed, 0 ignored**. Clippy
  `--workspace --all-targets`: exit 0, existing warnings only. Native Windows/RTX 4060 D3D12 ran.

Done this session (owner's UI priorities; all complete):
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
- Latest local test builds: `target/release/subcraft.exe` and `subcraft-preview.exe` (identical).
  Main screenshot:
  `docs/screenshots/ui-refinement-final.png`; per-feature evidence is in progress sessions 55–60.

Stops/limits:
- No partial work remains in the requested UI list. The larger brush roadmap is still partial.
  Phase 1 stops at `paint/src/stroke.rs::Stroke::write` (static source sampled after coverage);
  camera/per-stamp need stamp-dependent accumulation. `paint/src/target.rs::TargetImage::compute_tile`
  owns source snapshot/normal blending. Projection-frame/mirrored-tangent normals and async
  starter-pack brush destinations (`app/src/asset_apply.rs::Destination`) remain.
- Native tool switching/Flow/Hardness/typed Size were checked before the owner stopped Computer
  Use with Escape. No further Computer Use calls were made; later checks used native CLI
  screenshots. New grid/symmetry/scale gestures, tablet, mixed-DPI and other platforms are not
  hand-verified. Real egui input tests, GPU grid acceptance and paint texels at 75/95/150% pass.
- No failing/ignored tests remain. One initial async shelf-import polling test flaked under a
  concurrent build; full reruns passed without weakening it. Full 4K frame pacing is unmeasured;
  prior CPU 4K stroke-to p95 1.68ms/max 3.72ms is not whole-frame certification.
- Owner app windows/data were left untouched. The standard executable was refreshed only after
  its process had exited; its SHA256 matches the tested preview. Protected files were untouched.

Keep: dependency direction; tiled COW storage; one undo per stroke; immutable source snapshots;
shared RNM; one app brush settings/options store. Grid/UI size are display preferences only.
Project schema stays 4, brush preset version 2. See ADRs 0011–0015. Owner question for Phase 9:
future X color-swap conflicts with the existing X mirror shortcut; resolve when editing shortcuts.

Next 3 steps:
1. Manually smoke-test grid/symmetry popovers, scale/reset and pen/stencil after scaling when
   desktop input is resumed; use the latest preview build and record limits/screenshots.
2. Implement camera/per-stamp sources in Stroke with batching/symmetry/alpha/undo numeric tests.
3. Finish projection/mirrored-tangent normal reorientation, then follow the ordered roadmap for
   async brush drops, complex-model checks and full 4K paint→composite→upload profiling.
