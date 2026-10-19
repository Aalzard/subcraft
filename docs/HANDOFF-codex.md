# Handoff — high-poly baking repair (2026-10-19)

- Branch `feature/brush-roadmap`; implementation commit `274d144`. This handoff is in the final
  docs commit (`git log -1`). **423 workspace tests passed, 0 failed/ignored**, 9 added.
  Clippy workspace/all-targets exits 0 (pre-existing warnings). CARGO_INCREMENTAL=0,
  CARGO_PROFILE_DEV_DEBUG=line-tables-only; checks -j 1. No push/release or branch switch.

Done:
- `mesh`: UV-less high-poly import through import_mesh_for_baking/build_for_baking; low paint
  import stays strict. Existing import formats share the path; no automatic unwrapping.
- `bake`: total projection misses report alignment/cage failure; partial ID misses use neutral
  IDs instead of indexing the high-poly element array with low-poly triangle IDs.
- `app/bake_job.rs::poll` + `session.rs::show_baked_normal`: successful high Normal bake adds a
  live normal-only top fill, Inverse Detail RNM, undoable insertion, authored material/selection
  unchanged. Existing mesh-map links prevent duplicates. Poll waits while a brush stroke is
  active. Partial failures and worker disconnects surface errors. Maps keep existing separate
  non-history storage; undoing insertion does not undo the maps themselves.
- `ui/dialogs.rs`: persistent results/errors and clear cage/UV guidance; actual Bake-button
  dispatch covered by synthetic egui input. CLI --bake-high / --bake-size uses the same job;
  screenshots wait for the job/composite to complete. ADR-0019 explains the decisions.
- Native screenshots inspected: docs/screenshots/high-poly-bake-no-uv.png (all 7 maps at 1024,
  442,368-triangle UV-less high source, visible normal detail and completion) and
  high-poly-bake-misaligned.png (actionable failure, unchanged low). No mouse input.
- Updated local test app: E:\Applications\ArtCraft\SubCraft\target\debug\subcraft.exe.
  Owner's open target/release/subcraft.exe was left running; it still contains the old code.
  Published 0.1.7 is unchanged. Artwork originals and protected root files untouched.

No partial implementation or remaining failing/skipped tests. Initial headless tests needed
window sizing frames and texture-delta disposal; corrected and reran. Existing async image-save
UI test failed once during the interrupted run, then passed isolated and in full reruns; its
iteration-based wait may still be timing-sensitive. Not verified: owner's exact models (not
provided), native pointer Browse/Bake gestures, production FBX/GLB pairs, web runtime, 4K
interactive baking/frame pacing. Existing slider split-undo issue remains outside this change.
Keep baked links live/in UV space; do not flatten or silently double-apply existing references.
No new project schema: existing fill/source/blend serialization covers the new layer.

Next 3 steps:
1. Owner saves current work, opens the patched debug app and retries their exact low/high pair;
   then rebuild the regular release executable when it is no longer in use.
2. Validate production FBX/GLB/cage adjustments and a 4K native bake; consider reporting partial
   projection coverage. Custom cages/name matching/automatic alignment remain future work.
3. Resume layer-system-roadmap stage/halo contracts and tested neighborhood filters, followed
   by Compare Mask/anchors. Brush roadmap phase order remains unchanged.
