# Handoff — CORE-07 surface refinement (2026-10-19)

- Branch `feature/brush-roadmap`; artwork commit `7aca1b2`. This handoff is in the final docs
  commit (`git log -1`). No push, release, branch switch or production app changes.
- Tests: **414 workspace passed, 0 failed/ignored**, plus **5 independent creator tests passed**.
  Workspace and creator Clippy exit 0 with existing warnings. Required environment used:
  CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=line-tables-only; workspace checks -j 1.

Done:
- `artworks/CORE-07/source/main.rs`: 35% deeper high-poly panel relief with tested cage bounds;
  separate ceramic/cover/steel/enamel procedural roughness; 3 new editable scuff/dust/rust layers.
- Steel-only nonmetallic rust; scuffs protect graphics/lenses; numeric coverage/determinism tests.
- Separate 14-layer 4K `artworks/CORE-07-Refined/CORE-07.subcraft`; original project preserved.
  Seven real bakes (65.4s), PBR/Unreal exports, actual SubCraft GPU beauty/reverse images.
- Native 0.1.7 project opened and screenshot inspected: `docs/screenshots/core-07-refined-native.png`.
  No mouse input. Complete asset ZIP 78,101,902 bytes in refined folder; generated large assets
  ignored, source/validation/renders tracked. Original app window was not closed.
- Save/reopen exact document/composite equality; finite channels; max normal error 0.000778.
  11 export dimensions verified 4096 square; sampled clean armor roughness now 0.388–0.482
  versus original constant 0.478. Numeric reports are in refined folder.

No partial work, failing or skipped tests. Not checked: pointer/pen painting, interactive 4K
frame pacing, Unreal/Unity import, Linux/browser artwork rendering. Pre-existing slider
split-undo issue unchanged. No new schema or architectural decision requiring an ADR.
Keep normal bake in UV projection; neutral Height avoids double relief. PBR normals OpenGL,
Unreal DirectX, ORM R=AO/G=roughness/B=metallic. Color maps sRGB, data maps raw.
Protected root ZIPs, output/, generate_textures.py and __pycache__/ untouched; no cleanup.
Open owner question: preferred balance of clean ceramic and weathering after viewing new asset.

Next 3 steps:
1. Owner opens refined project, compares layers 12–14 and adjusts coverage/materials to taste.
2. Profile actual long strokes on this 4K layered asset; static generation is not frame pacing.
3. Resume layer-system-roadmap: stage/halo contracts, tested neighborhood filters, then Compare
   Mask/anchors. Brush phase order remains unchanged; keep app/paint independent of UI/GPU.
