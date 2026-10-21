# Claude session 95 (2026-10-21): see progress.md session 95. Commits afd8018, 8be56a4, 9950f57, bf7a428 + docs. 564 tests pass. Natural wear Level=amount / Spread=reach; brush-slot drops (issue 58); randomized compositor differential tests. Not hand-verified: wrap across hard edges, double-click Brush tab, physical drags. Issue 53 fix missing locally.

# Claude session 96 (2026-10-21): Clone + Smudge tools and camera projection painting; see progress.md session 96. 569 tests pass. Not hand-verified with a real mouse.

# Claude session 97 (2026-10-21): 15 new smart materials, 16 new smart masks, 5 new procedural generators (Dirt, Cracks, Streaks, Speckles, Pores); see progress.md session 97.

# Claude session 98 (2026-10-21): viewport post (tone map, bloom, AO), render/turntable export, emission glow/glare, Unity URP + Godot presets, Height + Bent Normal bakes; see progress.md session 98. 592 tests pass.

# Codex handoff — 2026-10-20, session 94

# Claude session 92 (2026-10-20): see progress.md session 92. Commits 12c55a2, 89d9ae9, 1420a6a + docs. 552 tests pass. Filter audit (task 4) unfinished.

Branch: feature/brush-roadmap. Last implementation commit: ee609d9d00400914b3e218a2c2aec7d6cc326ea8 (final documentation commit follows).
Workspace: 541 passed, 0 failed/ignored (3 new tests); workspace/all-target Clippy exits 0
with existing warnings. CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=line-tables-only,
--locked, -j 4. Desktop target/debug/subcraft.exe refreshed. No push/release.

Done:
- Reproduced imported PBR grain with Planks023A and Metal006 from D:/Textures.
- Added render-owned incremental GPU material mipmaps/trilinear sampling (ADR-0036).
- Preserved Nearest albedo, sparse uploads, authoritative texels, export and undo.
- Added numeric GPU/minification regressions and reproducible capture/profile examples.
- Visually checked native DX12 wood/metal before/after captures in docs/screenshots/
  material-filtering-{before,after}.png and material-metal-{before,after}.png.
- 120 synthetic 4K seven-channel updates + render: GPU-complete median/p95/max
  0.923/1.338/1.480 ms on RTX 4060/Vulkan. Excludes compositor/UI/input, not desktop FPS.

Stops/limitations:
- Implementation complete at render/src/lib.rs::ViewportRenderer::upload_channel and
  render/src/mipmap.rs::MipGenerator::update. No partial code or failing tests remains.
- No hand-driven input, original unsaved scene, other GPUs/platform matrix or production
  long-stroke/frame-pacing check. Automatic native screenshots are not manual interaction.
- Planks020 normalGL and Concrete044A displacement failed PNG decoding during candidate
  import. Originals untouched; cause not diagnosed. Successful wood/metal fixtures/logs are
  in %TEMP%/subcraft-viewport-aa. Helpers: ui/examples/material_viewport.rs and
  render/examples/perf_material_upload.rs.
- Prior Edge Wear/brush-wrap/filter audit remains in PROMPT-claude-edge-wear.md; untouched.
- Prior signing/vendor/CAPE dependencies remain unresolved; see release-security.md and
  progress session 93. No signed release, vendor verdict or security-clean guarantee.

Keep: linear area averages, odd-size coverage, exact mip-zero data, dirty-bound propagation,
Nearest LOD zero, cached GPU resources and render-only dependencies. Mips add ~1/3 material
GPU memory. Do not smooth authored maps or change roughness/height/export to hide aliasing.
Protected files and existing untracked subcraft-session92.patch retained.
Owner question: does the refreshed build resolve the original imported-material view?

Next 3:
1. Owner checks zoom/orbit/painting with original PBRs; profile complete layered 4K UI strokes
   and GPU memory before claiming frame pacing or all-material coverage.
2. Audit the two PNG decode failures with independent decoder/source validation.
3. Resume Claude's Edge Wear/brush/filter audit; retain pending release-signing safeguards.
