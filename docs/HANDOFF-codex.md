# Handoff — CORE-07 artwork (2026-10-19)

- Branch `feature/brush-roadmap`; last artwork code commit `5672db9`. This handoff is in the
  final docs commit (`git log -1`). No application crate changes, branch switch, push or release.
  Existing 0.1.7 release remains unchanged.
- Tests: **414 workspace passed, 0 failed/ignored**, plus **3 independent creator tests passed**.
  Workspace Clippy exits 0 (existing warnings); creator Clippy exits 0 (two style warnings).

Done:
- `artworks/CORE-07/source/main.rs` is a complete reproducible creator using existing SubCraft
  mesh/bake/app/material/export/render APIs. 3,072-triangle beveled cube; 442,368-triangle high
  source; seven real high-to-low maps at 4096², AO 64 samples; 11 editable PBR/mask layers.
- Curvature + Levels edge wear, AO + Levels dirt, cyan Emissive, stencil labels/hazard markings.
- Local `artworks/CORE-07/CORE-07.subcraft` and `CORE-07-assets.zip` (68,892,502 bytes), both
  OBJ models, PNG16 bakes, seven standard PBR + four Unreal maps, GPU beauty/reverse renders.
  Large generated assets are ignored; source, validation and beauty images are tracked.
- Save/reopen: exact document and composite equality. All channels finite; normal length max
  error 0.000802; 354,770 / 2,263,204 sampled covered texels have normal detail.
- Native 0.1.7 opened the complete 4K project; inspected `docs/screenshots/core-07-native.png`.
  Beauty images are from SubCraft's actual RTX 4060 renderer. Instructions in artwork README
  and getting-started. No mouse input; creation used engine APIs/app commands.

No partial artwork work or failing/skipped tests. No new design ADR or schema needed.
Not verified: pointer/pen painting, interactive 4K frame pacing, Unreal/Unity import, browser or
Linux artwork runtime. Neutral Height avoids duplicating the geometry already in the normal bake.
PBR normals are OpenGL; Unreal normals DirectX; ORM R=AO/G=roughness/B=metallic. Do not apply
sRGB to data maps. Pre-existing slider double-undo issue is unchanged (session 68).
Protected ZIPs, output/, generate_textures.py and __pycache__/ untouched. No build cleanup.

Next 3 steps:
1. Owner can open CORE-07 and try mask painting/generator edits; add a Paint layer for freehand.
2. Profile actual long strokes on this 4K layered asset; static generation is not frame pacing.
3. Resume the existing layer roadmap: stage/halo contracts, then tested neighborhood filters;
   Compare Mask and anchors follow. Keep app/paint independent of UI/GPU crates and COW storage.
