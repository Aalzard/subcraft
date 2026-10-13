# ADR-0001 — Technology stack: Rust + wgpu + egui

Status: Accepted (2026-10-14)

## Context

The research guide recommends C++23 + Qt 6 Widgets + Vulkan, on the grounds that the
professional imaging/DCC libraries (Qt docking, OpenImageIO, OpenColorIO, Embree, Assimp,
VMA, xatlas) are C/C++ and a Rust core would sit behind many FFI boundaries.

That argument is sound in the abstract. It was re-evaluated against the actual project
constraints:

| Need | C++ path (guide) | Rust path (chosen) | Notes |
|---|---|---|---|
| Docking UI | Qt 6 `QDockWidget` (LGPL/commercial review) | `egui` + `egui_dock` (MIT/Apache) | Tabs, splits, drag-to-dock, floating windows, serde layout persistence. |
| GPU | Vulkan 1.3 + VMA + GLSL→SPIR-V + SDK | `wgpu` 30 (Vulkan/DX12 backends) | No SDK; WGSL validated by naga; automatic barrier/state tracking removes the guide's "risk #2" (GPU synchronization) by construction. |
| Image I/O | OpenImageIO | `image` + `exr` | PNG/TGA/JPEG/TIFF/BMP/HDR/EXR read+write, 8/16/32-bit. |
| Mesh import | Assimp | `gltf` + `tobj` + `ufbx` | ufbx is a high-fidelity FBX loader (C, compiled by `cc`). Every importer normalizes into our own canonical mesh, as the guide demands. |
| Ray casting (bake/pick) | Embree | own SAH BVH (+ rayon) | Small, testable, deterministic; Embree remains an option behind the `bvh` API. |
| Tangents | — | `mikktspace` | Standard bake/render tangent basis. |
| Color management | OpenColorIO | built-in linear/sRGB/ACES-fit now | OCIO is the one real gap; it will enter through a narrow FFI boundary in `core::color` later. |
| Persistence | custom | `serde` + `zip` container | Versioned logical schema, atomic generation swap. |

Environmental facts that decided it:

* The machine has Rust 1.98 (MSVC) and Visual Studio build tools. It has no Qt, no Vulkan SDK,
  and none of OIIO/OCIO/Embree prebuilt; building those from source on Windows is hours of
  fragile setup before the first pixel.
* Every sibling ArtCraft application (PhotoCraft, CADCraft, EffectCraft, …) is a Rust/egui/wgpu
  workspace, so tooling, conventions and future shared crates align.
* The texture engine is heavily multi-threaded (tile-parallel painting, compositing, baking,
  background autosave). Rust's data-race freedom is a direct correctness win there.
* `cargo build` from a clean checkout is one command with no external SDK.

## Decision

* Language: **Rust 2024 edition**, Cargo workspace of focused crates.
* UI: **egui 0.36 / eframe** with **egui_dock 0.21** for the docking workspace.
* GPU: **wgpu 30** (the version egui-wgpu pins), Vulkan or DX12 backend chosen by wgpu.
* Shaders: WGSL embedded in the render crate, validated by naga at pipeline creation and in tests.
* Texture engine: CPU-authoritative tiled engine (see ADR-0003); GPU is used for display, with a
  GPU compositor/paint accelerator planned behind the same interfaces.

## Consequences

* No hardware sparse residency in wgpu: virtual texturing will be the software tiled
  implementation (which the guide notes is required as a fallback anyway).
* OCIO, MaterialX and OpenUSD integrations will need FFI or native parsers at their boundaries.
* egui is immediate-mode; panels must stay cheap per frame (no per-frame allocations of large
  data, thumbnails cached as textures).
