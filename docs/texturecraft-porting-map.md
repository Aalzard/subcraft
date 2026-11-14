# SubCraft → TextureCraft porting map

Written 2026-11-02 by a read-only survey session. Nothing was pushed, opened or posted. TextureCraft
(TC) was cloned shallow into `D:/texturecraft-survey` (one commit, `426aeff`, so `git log` carries no
history, and issues and PRs were not read: GitHub was not queried).

## Read this first

1. **TC is far ahead of what the brief assumed.** Its own `ROADMAP.md` (dated 2026-11-02) claims about
   94 % Painter parity, about 75 k lines of Rust in 11 crates, and already lists USD, Alembic, FBX, glTF,
   GPU baking, a GPU path tracer, brush presets, UDIM, symmetry (including radial), 2D-view painting and
   MaterialX. The survey is therefore not "SubCraft has a lot that TC lacks". Most rows below are
   "TC already has it". The real gap is small and specific.
2. **Its rules forbid the biggest SubCraft-only feature.** `AGENTS.md` ("Clean room"): "Painter file
   compatibility (`.spp`, `.spexp`, `.sbsar`) is a non-goal", and no proprietary-format files in the
   repo, not even as test fixtures. The `.spp` importer should not be proposed as a PR unless the
   owner changes that policy. Ask first, and expect a no.
3. **Everything I know about TC comes from its docs and greps of its source.** I did not build or run
   it. "Has" means the feature is documented in `docs/features.md` or `ROADMAP.md` and I found code for
   it. I did not verify the code works or how well. The ROADMAP percentages are the owner's own
   claims.

## Licence compatibility

TC: MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`). SubCraft: `license = "MIT OR
Apache-2.0"` in the workspace `Cargo.toml`, with an Apache-2.0 `LICENSE`. Same terms, so code written by
the user can move across both ways. Points to respect:

- **Contributor licensing.** Rafael's merged PRs (see `rafael-prs-handover.md`) and any other
  third-party contribution in SubCraft carry their authors' rights. Port only code that is the user's
  own, or that the contributor agrees to relicense into TC.
- **Assets.** TC needs a row in `ATTRIBUTION.md` for each bundled asset in the same PR ("No row, no
  merge"). Procedural content (smart materials, alphas, presets as data) is easiest. No image or
  texture may derive from Adobe products. SubCraft's `.spp`-derived calibration data and sample
  projects must not travel.
- **Brand.** `docs/brand/` is the ArtCraft trademark and is not open source. Irrelevant to code
  ports; keep SubCraft's name and branding out of the PRs.
- **From-scratch decoders** (JPEG 2000, 7z/LZMA, LZ4, HDF5 writer in `crates/mesh/src/spp/`) are the
  user's own code. Licence is fine; the policy in point 2 is the blocker, not the licence.

## Crate boundary mapping

| SubCraft crate | TC counterpart | Notes |
|---|---|---|
| `core` (ids, channels) | `core` (L0) | TC `core` also holds the document model and blend math. SubCraft splits that into `doc` and `compositor/blend.rs`. |
| `image` (`TiledImage`, I/O, dilate) | `texel` (sparse COW 64² tiles, padding, noise) | Same idea: 64² COW tiles. SubCraft stores f16; check TC's `tile.rs` before porting kernels. |
| `mesh` (BVH, import, topology, uv_raster, `spp/`) | `geom` (mesh, BVH, G-buffer, camera, primitives, unwrap) + `formats/mesh_import` | Import code goes to `formats`, geometry to `geom`. |
| `doc` (layers, masks, effects, History) | `core` (`layer.rs`, `project.rs`) + `engine` | TC undo is whole-document snapshots with COW tiles. |
| `materials` (noise, patterns, sample) | `texel/noise.rs` + `compose/pattern.rs`, `compose/eval.rs` | |
| `compositor` | `compose` (the CPU "oracle") | Filters live in `core/layer.rs` + `compose/eval.rs`, with the library in `engine/filter_lib.rs`. |
| `paint` | `paint` (brush, stroke, stamp, commit, presets, particles, text) | Closest 1:1 mapping. |
| `bake` / `render/ray_bake` | `bake` + `render/gpu_bake.rs` | TC already bakes on the GPU. |
| `project` (`.subcraft`) | `formats/project_file` (`.tcraft`) | Different schemas. Never port a file schema, only features. |
| `export` | `formats/export` + `ui-egui/export_templates.rs` | |
| `app` (Session, commands) | `engine` (Session plus a **command registry**) | Biggest structural difference. In TC every user-visible action is an `engine/src/*_cmds.rs` command with id, label, params doc, `enabled`, `run`; menus, CLI, MCP and the control channel dispatch by id. SubCraft uses `Command` enums and `Action`. A port is not a copy: each feature gets rewritten as a command plus tests, including the `panic_hunt` hostile-params test. |
| `ui` | `ui-egui` over `craft-ui` / `craft-window` | |
| `render` | `render` (wgpu 30; CPU+GPU path tracer) | |

Extra TC rules a port must obey (`AGENTS.md`): no `unwrap/expect/panic/todo/unreachable` in non-test
code (deny lints on every crate root), no `unsafe`, hostile-input caps on allocations, `cargo xtask
layers` enforces layering, new crates must be registered in `xtask/src/layers.rs`, and egui may only be
used in `ui-egui`. SubCraft code uses `anyhow`, `parking_lot` and `unwrap` in places, so all ported code
needs a lint pass. Also, TC states "Rust only" and "copy, don't depend" unless the code is in craft-libs.

## Shared-library dependencies

- **`storytold/craft-libs`** (`craft-ui`, `craft-window`, pinned by rev `f8f8829…`) and **forks of egui
  0.37 and winit** (`storytold/egui`, `storytold/winit`), patched in via `[patch.crates-io]`.
  SubCraft is on egui 0.36 / eframe / egui_dock 0.21. **No SubCraft UI code ports as is.** Widgets must be
  rewritten against `craft-ui`'s palette, icon buttons and docking. SubCraft's `ui` crate (15 k lines) is
  essentially not portable; only engine and paint logic is.
- **`storytold/craftrules`** (`../craftrules`, standards and `AGENTS.md`) and **`storytold/craft-fonts`**
  are referenced but were not cloned (not needed for the map). They are a sibling checkout the
  contributor is expected to read. I did not read `craftrules/AGENTS.md`, so any further standards there
  are unknown.
- TC's `Cargo.toml` already has `lz4_flex`, `flate2`, `zip`, `half`, `exr`, `image`, `gltf`, `tobj`,
  `mikktspace`, `quick-xml`, `rmcp`, `harfrust`/`skrifa`. SubCraft's `ufbx` (FBX) is not there; TC reads
  FBX with its own `formats/mesh_import/fbx.rs`. No new dependency is needed for the small PRs below.

## Feature matrix

Legend: **Has** = documented and present in code, **Partial** = present with a stated or
suspected limit, **Missing** = not found. "SC" = SubCraft.

| Area | SubCraft | TextureCraft | Evidence (TC paths) |
|---|---|---|---|
| Painter `.spp` import (reader, JP2/7z/LZ4, stack rebuild) | Yes (`crates/mesh/src/spp/`, `crates/app/src/spp_*.rs`, about 7.8 k lines) | **Missing, by policy** | `AGENTS.md` "Clean room" lists `.spp` as a non-goal |
| `.sbsar` archive reading | Reads archives for embedded bitmaps | **Missing, by policy** | same |
| glTF / FBX / OBJ import | Yes | **Has** | `crates/formats/src/mesh_import/{gltf,fbx,obj}.rs` |
| USD import | Branch `claude/usd-import-*` (not on the checked-out branch) | **Has** (usda/usdc/usdz) | `mesh_import/usd/` |
| Alembic import | Branch `claude/alembic-import-*` | **Has** (Ogawa; first sample only) | `mesh_import/abc/` |
| COLLADA / PLY / STL | Not seen in SC | **Has** | `mesh_import/{dae,ply,stl}.rs` |
| GPU path tracing | Branch `claude/path-tracing-*` | **Has** (GPU and CPU, denoiser, F10 mode) | `render/src/{gpu_trace,trace}.rs`, `ui-egui/src/render_mode.rs` |
| GPU baking | AO, thickness, curvature (`render/ray_bake.rs`, ADR-0039) | **Has**, covers more maps | `render/src/gpu_bake.rs`, `bake/` |
| Baking: high-poly, cage, match by name, skew | Yes, but match by name is "missing" in SC's own list | **Has** | `bake/src/skew.rs`, `docs/features.md` |
| Auto-rebake | No | **Has** | `engine/src/auto_rebake.rs` |
| Brush engine core (size/flow/spacing/hardness/jitter/angle) | Yes | **Has** | `paint/src/brush.rs`, `stamp.rs`, `stroke.rs` |
| Lazy mouse | Yes | **Has** (`lazy` 0..500) | `paint/src/brush.rs:203` |
| **Stroke stabilizer**: weighted-average smoothing, rope, catch-up (SC session 109) | Yes | **Partial**: only `lazy`; grep found no weighted-average smoothing or catch-up | `paint/src/path.rs` (unread in detail) |
| **Scatter / particle brush with fall** | Yes (session 109) | **Has** a particle brush | `paint/src/particles.rs` |
| Smudge, clone | Yes | **Has** | `paint/src/pickup.rs` |
| Smudge with material (SC session 109) | Yes | **Unknown**; not checked | `paint/src/pickup.rs` |
| Brush presets | 13 built in, user presets | **Has** 18 built in plus user presets | `paint/src/presets.rs`, `engine/src/brush_preset_cmds.rs` |
| Brush alphas | Procedural + import | **Has** 67 procedural | `paint/src/alphas.rs` |
| Projection with stencils | Yes | **Has** | `engine/src/painting.rs` |
| Text brush / decals | Not in SC | **Has** | `paint/src/text.rs`, `engine/src/decal_cmds.rs` |
| Bezier / editable pen paths | Yes (ADR-0008) | **Has** path tool | `engine/src/path_tool.rs` |
| Symmetry (mirror, radial) | Mirror, plus radial in Rafael's PR | **Has** X/Y/Z mirror and radial | `docs/features.md` |
| 2D-view painting | Yes | **Has** | `docs/features.md` |
| Pen pressure | Yes, with a second model in skipped PR #69 | **Has** pressure; tilt not available | `ROADMAP.md` "Painting tools" |
| Layer stack: paint/fill/folder, 32 blend modes, masks, anchors | Yes | **Has**, plus instance layers | `core/src/layer.rs`, `engine/src/layer_cmds.rs`, `instance_cmds.rs` |
| Effect drag reorder / copy-paste of effects | Yes (session 90) | **Has** | `engine/src/effect_clipboard.rs` |
| Per-effect stage thumbnails | Yes (session 89, ADR-0033) | **Unknown** (not checked) | — |
| Geometry masks / parts workflow | SC `parts.rs`, `part_workflow.rs`, `part-mask` branch | **Has** | `ROADMAP.md` "geometry masks" |
| Filters | About 18 incl. Median, Mask Outline, Bilinear/Nearest, Gradient Map | **Has** 31, incl. Slope Blur, Glow, Mosaic, MatFinish | `engine/src/filter_lib.rs:25` |
| **Median filter** | Yes (`doc/src/filter.rs:120`) | **Missing** (not in `FILTERS`) | `filter_lib.rs` |
| **Mask Outline filter** | Yes | **Missing** (not in `FILTERS`; Bevel is) | `filter_lib.rs` |
| Smart materials | 34 | **Has** 21 | `docs/features.md`, `engine/src/material_lib.rs` |
| Smart masks / generators | 36 smart masks, bespoke Edge Wear spread (ADR-0027/28) | **Has** 18 smart masks, `smart_masks.rs`; edge wear and dirt generators | `engine/src/smart_masks.rs` |
| **Extra built-in content** (SC's extra 13 smart materials, 18 smart masks) | Yes | **Missing** by count; could be ported as data | same |
| Procedural materials | Presets (16 plus) | **Has** 63 | `engine/src/materials.rs` |
| Export presets | 4 templates, engine presets (session 98) | **Has** 10 plus user templates, ICC/colour spaces | `formats/src/export/`, `ui-egui/src/export_templates.rs` |
| UDIM | Yes (ADR-0037) | **Has**, per-tile resolution | `engine/src/uv_tile_cmds.rs` |
| Albedo nearest/bilinear viewport sampling (ADR-0020) | Yes | **Unknown** (grep shows only a mip-related hit in renderer) | `render/src/renderer.rs` |
| Viewport mipmaps (ADR-0036) | Yes | **Unknown** | — |
| MaterialX / MaterialCraft | Not in SC | **Has** | `formats/src/materialx*.rs`, `mcmat.rs` |
| Agent control (MCP, CLI, control channel) | No | **Has**, central | `crates/automation/` |
| Localisation | No | **Has** 10 languages | `ui-egui/src/i18n/` |
| Docking / floating panels, titlebar | egui_dock | **Has** | `ui-egui/src/{dock,docking,titlebar}.rs` |
| Auto-updater | Yes (`app/src/updater`, ADR-0007) | Not found; TC uses releases/packaging | — |
| Crash recovery of unsaved work | SC `recovery.rs` | **Partial/unknown**: autosave exists, recovery not found | `ROADMAP.md` "autosave" |
| Browser/WASM build | Removed (ADR-0038) | **Has** | `apps/texturecraft-web` |
| Project file | `.subcraft` zip v1 | `.tcraft` | `formats/project_file/` |

## What is missing, ranked

Value = what a TC user gets. Effort = how hard to port into TC's structure. Both are my estimates.

| Rank | Item | Value | Effort | Why |
|---:|---|---|---|---|
| 1 | **Median filter** | Low-medium | **XS** | Self-contained kernel. Add `Filter::Median` to `core/layer.rs`, evaluation to `compose/eval.rs`, a row in `filter_lib.rs`, and tests. |
| 2 | **Mask Outline filter** | Low-medium | S | Same plumbing as Bevel (which TC has). SC code is in `compositor`; ADR-0032. |
| 3 | **Stroke stabilizer** (weighted-average smoothing + rope + catch-up) | Medium-high for tablet users | S-M | TC has only `lazy`. Pure function of the stroke path, so it fits `paint/src/path.rs` and keeps the dependency direction. Needs a settings field, a command and a golden test. |
| 4 | **Smudge with material** | Medium | S-M, **only if** TC lacks it (not verified) | Check `paint/src/pickup.rs` first. |
| 5 | **Extra procedural smart materials / masks as data** (SC's roughly 13 and 18 extra) | Medium | S per batch | Pure data. TC lists 21 and 18. Each needs a thumbnail, i18n label rows and a golden/numeric check. Overlap with TC's names is likely, so de-duplicate. |
| 6 | **Crash recovery** (SC `recovery.rs`) | Medium | M | Depends on TC's autosave design (`autosave` in ROADMAP). Verify TC's behaviour before proposing. |
| 7 | **Effect-stage thumbnails** (ADR-0033) | Low-medium | M-L | Touches `engine/preview.rs` and `ui-egui` heavily; UI part cannot be ported. |
| 8 | **Viewport albedo nearest/bilinear toggle** (ADR-0020) | Low (pixel-art users) | S | A render/display option, only if TC lacks it. |
| — | **`.spp` import** | **Highest for Painter migrants** | Very large | Blocked by TC policy. Not recommended unless the owner reverses it. |
| — | **UI crate** | — | — | Not portable: egui 0.36 vs the forked 0.37 plus `craft-ui`. |
| — | Updater | Low | L | TC ships through its release workflow; a self-updater is a policy matter. |

I could not find anything SubCraft has in rendering, baking, formats or layers that TC lacks, because
TC has already matched them. SubCraft's value to TC is **a small number of tool and filter details**, not
major subsystems.

## Proposed PR sequence

Each PR is one feature, one `*_cmds.rs` command at most, with tests, with no new dependencies, and
following TC's no-panic lints. Before each, run `cargo xtask layers`, `cargo fmt`, `clippy`, and TC's
`panic_hunt` test. Start with the smallest so the ArtCraft owner can calibrate review before the larger
ones.

1. **Median filter**: `Filter::Median { radius }`; evaluation; `filter_lib` row; numeric tests (a salt-and-pepper image
   is cleaned, a constant image is unchanged); update `docs/features.md` filter count. Roughly 150 lines.
2. **Mask Outline filter**: as above, with numeric tests on a rectangle mask.
3. **Stroke stabilizer, step 1**: weighted-average smoothing in `paint`, setting plus command, golden-path
   test (smoothed path length is less than the raw path length, endpoints preserved).
4. **Stroke stabilizer, step 2**: rope and catch-up mode, if the owner wants it after step 1.
5. **Smudge with material**, after confirming the gap by reading `pickup.rs`.
6. **Smart material / smart mask data batches**, 5-6 items per PR, each with an `ATTRIBUTION.md` row if
   anything beyond code is added.
7. **Crash recovery**, only after reading TC's autosave design.

Before opening any PR the user should talk to the owner: "is a stabilizer wanted", and "does TC want
Painter import at all". The policy text suggests the second answer is no.

## Uncertainty

- Only one commit of TC was cloned, so I do not know recent churn, open PRs, issues or who is working on
  what. A stabilizer or median filter may already be in a branch.
- "Has" rows are based on docs and file/greps, not execution. TC is large and the docs may overstate.
- Rows marked Unknown were not checked: smudge with material, stage thumbnails, albedo sampling modes,
  recovery. Check these before committing to PRs 4-7.
- SubCraft's own branch list shows USD, Alembic and path-tracing work on side branches. I did not
  inspect their state. They matter only as references, since TC already ships all three.
- The SubCraft filter set was read from `crates/doc/src/filter.rs` partially (the enum's first 50 lines of
  variants); there may be filters beyond what I listed.
- Size and line counts come from `wc -l` across `*.rs`, rough.
