# SubCraft

[![Download for Windows](https://img.shields.io/badge/Download-Windows%20installer-ff7a1a?style=for-the-badge&logo=windows&logoColor=white)](https://github.com/Aalzard/subcraft/releases/latest)
[![Latest release](https://img.shields.io/github/v/release/Aalzard/subcraft?style=for-the-badge&label=latest&color=2b2b2b)](https://github.com/Aalzard/subcraft/releases/latest)

An independent, open-source 3D texture painting application in the spirit of Substance Painter,
written in Rust. Paint directly on 3D models with layered PBR materials, masks, generators and
smart materials, bake mesh maps, and export game-ready textures.

![SubCraft](docs/screenshots/ui-redesign.png)

> **Status: early but usable.** The core engine, painting, baking, smart materials, save/load and
> export all work and are covered by tests. Expect rough edges, missing features and breaking
> changes to the project file format. See [`docs/progress.md`](docs/progress.md) for what is done
> and what comes next.

## Features

- **Layer stack**: paint layers, fill layers (uniform, procedural, image and mesh-map sources) and
  folders, with 32 blend modes and opacity per channel. Right-click a layer to duplicate, delete or
  save it to your library.
- **PBR channels**: base color, roughness, metallic, normal, height (shown as bumps), ambient occlusion.
- **Your own textures**: use any image (PNG, JPG, TGA, TIF, EXR…) in a fill layer with UV, tri-planar or
  planar projection, or **import a whole texture set** (base color, roughness, normal, height…) as one
  material: files are matched by name, glossiness and DirectX normals are converted automatically.
  Images are stored inside the project, so it stays portable.
- **Masks**: paintable masks with stacked effects: Edges (curvature), Cavity (AO), Directional,
  Position gradient, noise fills, **images** (e.g. a logo), Levels and Invert.
- **Painting**: brush and eraser on the 3D model with tips (alphas), symmetry, lazy mouse, jitter,
  **stencils** (paint through an image laid over the view) and polygon fill (triangle, mesh part,
  UV island). Save your brush setups as **presets**.
- **Baking**: normal (from an optional high poly through a cage), world-space normal, AO, curvature,
  thickness, position and ID maps, on the CPU in the background.
- **Shelf**: built-in materials, smart materials (Worn Painted Metal, Rusty Iron, Old Wood…),
  smart masks, brush tips and lighting environments (including HDRI import). Search and filter by
  category, and **drag** items onto the model or a layer.
- **Your library**: imported tips and HDRIs, your saved smart materials, smart masks and brush presets
  live in `Documents\SubCraft\Library` and are back every time you start SubCraft.
- **Viewport**: real-time PBR preview with studio or HDRI lighting, plus a 2D UV view of every channel
  and baked map.
- **Projects and export**: `.subcraft` project files; export templates for PBR metal/rough, Unreal
  (ORM + DirectX normal), Unity HDRP (mask map) and glTF.
- **Mesh import**: OBJ, glTF / GLB and FBX.

## Download (Windows)

1. Open the [latest release](https://github.com/Aalzard/subcraft/releases/latest) and download
   **`SubCraft-<version>-setup.exe`** under **Assets**.
2. Run it. Windows SmartScreen may say "Windows protected your PC" because the installer isn't
   code-signed yet: click **More info → Run anyway**.
3. Follow the steps (you can choose "Install for me only" if you don't have admin rights), then
   start SubCraft from the Start Menu. Uninstall it any time from **Settings → Apps**.

Prefer no installer? Download **`SubCraft-<version>-windows-x64.zip`** instead, unzip it and run
`SubCraft.exe`. Either way, any GPU with DirectX 12 or Vulkan works.

## Building from source

You need a recent stable [Rust toolchain](https://rustup.rs) (1.95 or newer) and a GPU with
Vulkan, Metal or DirectX 12 support.

```sh
git clone https://github.com/Aalzard/subcraft.git
cd subcraft
cargo run --release
```

The app opens with a sample crate. Try `cargo run --release -- --demo` for a ready-made layer
stack, or pass a mesh / project file: `cargo run --release -- path/to/model.fbx`.

Step-by-step Windows setup, the daily workflow and building the `.exe` yourself:
[`docs/development.md`](docs/development.md).

On Linux you may need the usual windowing libraries (for example on Debian / Ubuntu:
`libxkbcommon-x11-0 libegl1`) and a Vulkan driver.

### Controls

| Action | Input |
| --- | --- |
| Paint / erase | Left mouse (B = brush, E = eraser) |
| Straight line | Shift + click |
| Orbit | Alt + left mouse, or right mouse |
| Pan | Middle mouse |
| Zoom | Mouse wheel, or Alt + right mouse |
| Brush size | `[` and `]` |
| Move / rotate / scale stencil | Hold S + middle / left / right mouse |
| Paint without the stencil | Hold N |
| Apply a shelf item | Click it, or drag it onto the model or a layer |
| Bake mesh maps | Ctrl + B |
| Export textures | Ctrl + Shift + E |
| Undo / redo | Ctrl + Z / Ctrl + Y |

Help → Keyboard shortcuts lists all of them.

## Project layout

SubCraft is a Cargo workspace of small crates with a strict dependency direction (lower crates
never depend on higher ones, and only the rendering and UI crates know about the GPU or egui):

```
core ← image ← mesh / doc ← materials ← compositor / paint / bake ← project / export / assets ← app ← ui ← apps
```

| Crate | Purpose |
| --- | --- |
| `crates/core` | IDs, channel kinds, shared math types |
| `crates/image` | Tiled copy-on-write texel storage, image I/O |
| `crates/mesh` | Mesh model, importers, tangents, BVH, UV rasterization |
| `crates/doc` | Document model: texture sets, layers, masks, undo history |
| `crates/materials` | Procedural patterns, projections, sampling |
| `crates/compositor` | CPU layer compositing with dirty-tile caching |
| `crates/paint` | Brush engine, stroke placement, symmetry |
| `crates/bake` | Mesh-map baking |
| `crates/project`, `crates/export`, `crates/assets` | Project files, texture export, built-in assets |
| `crates/app` | Session and commands (all editing goes through here) |
| `crates/render` | wgpu viewport renderer |
| `crates/ui` | egui interface |
| `apps/subcraft` | The executable |

Design decisions are recorded in [`docs/adr`](docs/adr).

## Contributing

Contributions are very welcome: bug reports, feature ideas, docs and code. Please read
[`CONTRIBUTING.md`](CONTRIBUTING.md) first; it explains how to build, test and structure changes.
Everyone taking part is expected to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

SubCraft is dual-licensed under either of

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
SubCraft by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.

Bundled third-party assets keep their own licenses: the [Inter](https://rsms.me/inter/) font
(SIL Open Font License 1.1) and [Phosphor Icons](https://phosphoricons.com) (MIT), both in
`crates/ui/assets/fonts` with their license files.
