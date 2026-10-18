# Handoff — Claude session 69 (2026-10-18): interactive performance

- Same branch, nothing pushed. Owner asked for immediate slider response (“even at 1000”).
- Done (ADR-0018, progress session 69): mimalloc, exact procedural field cache, cached noise
  gradients, parallel writes + changed-tile uploads, 10 ms per-frame compositing budget shared
  by all panels with round-robin pending tiles, `--profile-edits` diagnostic, perf example.
- Real app (command-driven, not mouse): ~15 ms frames at 1K/2K, 17 ms at 4K while editing.
- Not done: worker-thread compositing (cold fields still cost ~40 ms frames during a 4K
  warm-up), GPU kernels, reduced-resolution drag previews. 404 tests pass.

---

# Handoff — Claude session (2026-10-18, sessions 67–68)

- Branch `feature/brush-roadmap` (nothing pushed). Code commits: `bd43cd7` (thumbnails/mask
  Properties, Codex's slice), `812081c` (content effect model/eval/schema 5), `add8bc5`
  (effect selection + Paint-effect strokes), `c012109` (effect rows + Properties UI). Docs
  are the last commit (`git log -1`).
- `cargo test --workspace`: **400 passed, 0 failed, 0 ignored**. Clippy workspace/all targets:
  exit 0, pre-existing warnings only. Release `target/release/subcraft.exe` rebuilt.
- Done: Codex's thumbnail/mask slice committed (owner hand-tested it). Separate content effect
  stacks (Paint, Fill, Levels) with per-channel blend, schema 5 (v1–4 load unchanged), typed
  effect selection, strokes into content and mask Paint effects (fill layers too), nested
  effect rows and effect Properties. ADR-0017. 62 accidentally deleted screenshots restored.
- Hand-checked (owner allowed mouse control): Paint effect on a fill layer + undo/redo, Levels
  Properties, Back to layer, mask Paint effect strokes. Found/fixed slider overflow. Not by
  hand: save/reopen, Fill effect settings, row menus, 4K effect strokes. Pre-existing: one
  native slider drag can create two undo entries.
- Known gaps: folders reject content effects (need stage graph); Fill effects only offer
  Uniform/Procedural sources; effect rows have icons, not thumbnails; no drag reorder of
  effect rows; unselected layers do not show their stacks; smart-material save/load with
  content effects is untested. No filters/Compare Mask/anchors yet.
- Disk: E: had 439 MB free; `cargo clean --profile dev` freed 19 GB. Use
  `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=line-tables-only` for tests.

Next 3 steps:
1. Hand-check save/reopen with effects, Fill effect settings and the row menus; look at the
   double undo entry from one slider drag (pre-existing, fill sliders too).
2. Effect rows: drag reorder, per-effect thumbnails (reuse `app::layer_previews` with an
   effect key), show stacks for unselected layers via expand/collapse; smart-material test.
3. ADR-0016 stage graph: neighborhood/halo evaluation, then Blur/Sharpen/etc. with goldens.

---

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
