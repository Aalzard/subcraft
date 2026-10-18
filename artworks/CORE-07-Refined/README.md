# CORE-07 — refined surface treatment

Open `CORE-07.subcraft` in SubCraft 0.1.7 or newer. This is a separate 4096² project;
the original CORE-07 project is preserved alongside it.

The panel seams, service covers, vents and fasteners have 35% stronger high-poly relief,
rebaked into seven mesh maps with 64-sample ambient occlusion. Ceramic, ivory covers,
steel and hazard enamel have independent procedural roughness ranges for varied reflections.

Fourteen editable layers include the original curvature wear/AO grime, plus broken panel-edge
scuffs, settled dust and restrained steel oxidation. Rust is nonmetallic and confined to exposed
steel hardware. Graphics and status lenses are protected from the added edge scuffs.

Select a layer's content thumbnail to adjust its color, metallic or roughness source.
Select its mask thumbnail to adjust coverage. Toggle layers 12–14 independently to compare
the surface treatment. Layers 10–11 retain editable Generator + Levels effects.
Lower mask/effect opacity to reduce weathering. Save a copy before experimenting.

`models/` contains low/high OBJ meshes; `bakes/` contains seven PNG16 mesh maps.
`textures/PBR/` contains seven exported maps (OpenGL normals); `textures/Unreal/` contains
four maps including DirectX normals and packed ORM (R=AO, G=roughness, B=metallic).
Base Color/Emissive are sRGB; other maps are raw data. Height is neutral because the panel
relief is already baked into Normal. Do not add that relief a second time.

Beauty/reverse images are rendered by SubCraft's own GGX renderer. `validation.json` records
full save/reopen equality and finite/normal acceptance. The native app screenshot is in
`renders/CORE-07-in-app.png`. Pointer painting and game-engine import were not checked.

Rebuild using `../CORE-07/source/Cargo.toml` from the SubCraft repository root:

```powershell
$env:CARGO_TARGET_DIR = (Join-Path (Get-Location) 'target')
cargo test --release --manifest-path artworks/CORE-07/source/Cargo.toml
cargo run --release --manifest-path artworks/CORE-07/source/Cargo.toml -- artworks/CORE-07-Refined
```

The asset ZIP includes a recipe copy in `source/`; its relative crate dependencies require
placing it at the same depth inside a SubCraft checkout. Five recipe tests check UV layout,
material landmarks, mesh frames, numeric relief/cage bounds, and restrained localized weathering.
