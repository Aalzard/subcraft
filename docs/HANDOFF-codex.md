# Handoff to Claude — 2026-10-18

- Branch: `feature/brush-roadmap`, from `origin/main` `cef93c9`; nothing pushed/released.
- Last code commit: `8b4e56ba5a504d7f9f9f327047059ac7971279cb`; this file is in the final
  documentation commit (`git rev-parse HEAD`).
- `cargo test --workspace`: **377 passed, 0 failed, 0 ignored**. Clippy
  `--workspace --all-targets`: exit 0, existing warnings only. Native Windows/RTX 4060 D3D12 ran.

Done this session (63):
- `ui::app::follow_layer_selection` reveals Brush for paints and Properties for fills/folders/
  paths/deselection. `workspace` checks before/after real dock drawing; manual tab choices stay
  until selection changes. Three pointer/numeric/history/layout regressions added.
- Native verified: Layer 1 → Moss Properties, channel edit changes fill while paint stays,
  undo, Wood Properties shows its own values, Layer 1 returns to Brush. Evidence:
  `docs/screenshots/inspector-{fill-selected,fill-edited,paint-return}.jpg`.
- Latest normal executable: `target/release/subcraft.exe`, SHA256
  `6D04251D4E54A0DAC618E1C6C15633F121F0C11809761AF825BD14FCD96D276E`.
  No new binary variant created. Preview/panels/toolbar executables are obsolete.
- Demo remains open with test edit undone. Test recovery/library live in
  `%TEMP%/subcraft-inspector-native`; no owner project file edited. ADR-0012 updated.

Prior toolbar/panel/grid/symmetry/UI-scale work is complete; see sessions 54–61.

Stops/limits:
- No partial work remains in the requested UI list. The larger brush roadmap is still partial.
  Phase 1 stops at `paint/src/stroke.rs::Stroke::write` (static source sampled after coverage);
  camera/per-stamp need stamp-dependent accumulation. `paint/src/target.rs::TargetImage::compute_tile`
  owns source snapshot/normal blending. Projection-frame/mirrored-tangent normals and async
  starter-pack brush destinations (`app/src/asset_apply.rs::Destination`) remain.
- Not hand-verified this session: restored projects, folders/paths, floating inspectors, native
  redo, tablet/mixed-DPI, other platforms, or previous grid/symmetry/scale gestures.
  Native accessibility element-click missed a scaled checkbox; screenshot coordinates worked.
  Accessibility bounds at 95% need investigation. No failing/skipped tests remain.
- Session 62 cleanup remains blocked: automatic approval review rejected both bulk cleanup
  and explicit target/debug deletion (“blocked by policy”). No artifacts were deleted. Do not
  bypass that restriction. Historical screenshot deletions remain unstaged; protected files
  untouched. Source/asset files and project data must be retained.
- No failing/ignored tests remain. One initial async shelf-import polling test flaked under a
  concurrent build; full reruns passed without weakening it. Full 4K frame pacing is unmeasured;
  prior CPU 4K stroke-to p95 1.68ms/max 3.72ms is not whole-frame certification.

Keep: dependency direction; tiled COW storage; one undo per stroke; immutable source snapshots;
shared RNM; one app brush settings/options store. Grid/UI size are display preferences only.
Project schema stays 4, brush preset version 2. See ADRs 0011–0015. Owner question for Phase 9:
future X color-swap conflicts with the existing X mirror shortcut; resolve when editing shortcuts.

Next 3 steps:
1. Smoke-test restored fill projects, pen/folder inspectors and floating docks; investigate
   native accessibility hit bounds at 95% without changing paint coordinate conventions.
2. Implement camera/per-stamp sources in Stroke with batching/symmetry/alpha/undo numeric tests.
3. Finish projection/mirrored-tangent normal reorientation, then follow the ordered roadmap for
   async brush drops, complex-model checks and full 4K paint→composite→upload profiling.
