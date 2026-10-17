# Handoff to Claude — 2026-10-18

- Branch: `feature/brush-roadmap`, from `origin/main` `cef93c9`; nothing pushed/released.
- Last implementation commit: `44fae8df34087bf7daac794e0323eaf4b301a9ff`; preceding perf commit
  `b00ad54`. This handoff is in the final documentation commit (`git rev-parse HEAD` after checkout).
- `cargo test --workspace`: **352 passed, 0 failed, 0 ignored** (11 new). Clippy
  `--workspace --all-targets`: exit 0, existing warnings only. Debug app/release example ran.

Done:
- Conservative UV tile bounds; bit-exact comparison against original full-triangle coverage.
- Per-channel brush images, whole-material import, UV/planar/triplanar transforms/addressing.
- Normal Replace/Detail with normalized output, shared core RNM; DX/gloss conversion reused.
- Brush Choose/Replace/Clear and imported texture drops; portable image-embedding v2 presets.
- Native D3D12 normal paint/preview/Detail/undo/redo/save on imported OBJ; three
  `docs/screenshots/brush-projection-normal-*.jpg` images. Full checklist in brush-roadmap.md.

Stops/limits:
- Phase 1 partial; no Phase 2+ work. `paint/src/stroke.rs::Stroke::write` samples static
  sources after coverage. Camera/per-stamp need stamp-dependent source accumulation, not this
  post-coverage method. `paint/src/target.rs::TargetImage::compute_tile` owns snapshot blending.
- Projection-frame / mirrored-UV tangent-vector reorientation is missing, as in fill sampling.
  `ui/src/panels/brush_material.rs::show` accepts imported Texture drops only; starter async
  destinations in `app/src/asset_apply.rs::Destination` / `ui/src/asset_preview.rs` target fills.
- Native automation yielded dabs rather than certifying a continuous long drag. No hand check
  of shelf drops, per-channel dialogs, multi-map picker, UV/triplanar or preset UI. No tablet,
  browser/Linux/macOS or complex production model checks. No failing/skipped tests left.
- 4K CPU textured stroke-to: p95 1.68 ms, max/cold 3.72 ms, 1,000 inputs / 164 ms total;
  many inputs emit no stamp. Reproducer `cargo run -p subcraft-app --release --example
  brush_projection -- <scratch-dir>`. Whole-frame compositor/upload pacing remains unmeasured.

Keep: architecture dependency direction; COW tiled rasters; pre-stroke immutable source capture;
one undo per stroke; image alpha × capped coverage; shared RNM. Tool state uses preset v2, not
new document source data, so project schema 4 stays unchanged. See ADR-0011. Do not touch the
owner's untracked ZIPs, output/, generate_textures.py or __pycache__/.

Open owner questions (not blocking): should whole-material brush import offer to add missing
texture-set channels (currently reports/skips), and should Detail optionally sample lower layers
(currently this paint layer only)?

Next 3 steps:
1. Implement camera/per-stamp source accumulation with batch-split, symmetry, alpha and undo
   golden tests; extend preset compatibility without introducing UI/GPU dependencies.
2. Complete tangent-vector reorientation and starter-pack async brush destinations; run native
   channel/preset/drop and complex-model checks with screenshots.
3. Profile full 4K paint→composite→upload frames, then finish Phase 1 before the top bar/guides.
