# CORE-07 — sci-fi service cube

A cube-based, mechanically detailed PBR prop made with SubCraft's own mesh builder, high-to-low
baker, layer/mask/generator system, project serializer, exporter and GGX renderer. No AI-rendered
mock-up or third-party beauty render is used.

## Open and edit

Open **CORE-07.subcraft** in SubCraft 0.1.7 or newer. The working texture set is **4096 × 4096**.
The 11 named layers separate the chassis, ceramic armor, ivory service plates, rubber seams,
steel fasteners, hazard stripes, markings, cyan status lenses, edge wear and cavity dirt.
Select a content thumbnail to change its material; select its mask to edit coverage.

The last two masks have editable **Generator + Levels** stacks. Edge wear reads baked
curvature; grime reads baked AO. They are deliberately restrained. The cyan lenses use an
actual Emissive channel. These are editable masks/fill layers, not one flattened base-color map.

## Files and material setup

- `models/CORE-07-low.obj`: 3,072 triangles, beveled silhouette and six separate square UV
  islands with matching texel density and generous gutters. Its MTL references the PBR maps.
- `models/CORE-07-high.obj`: 442,368 triangles with geometric panel recesses, vent slots,
  fan detail, countersinks and fastener heads. This is the real high-poly bake source.
- `bakes/`: seven 16-bit mesh-map PNGs: tangent/world normals, AO, curvature, position,
  thickness and ID. AO uses 64 hemisphere rays per covered texel; projection uses a cage.
- `textures/PBR/`: separate BaseColor, Roughness, Metallic, Normal, AO, Emissive and Height.
  BaseColor/Emissive are sRGB; all other maps are linear/raw data. **Normal is OpenGL (+Y)**.
  Height is neutral: geometric relief already lives in the normal bake, so do not add it twice.
- `textures/Unreal/`: the app's Unreal export, including packed ORM and DirectX (-Y) normals.
  ORM is R=AO, G=Roughness, B=Metallic. Import it with sRGB disabled.
- `renders/`: front and reverse beauty images from SubCraft's own real GPU PBR renderer.
- `validation.json`: measured checks, resolution, triangle counts and bake settings.

OBJ/MTL support for roughness, metallic, emission and normal maps varies between applications.
If those maps do not connect automatically, connect the separate files using the rules above.
The SubCraft project embeds the model, masks and baked maps and is portable on its own.

## Reproduce

The current recipe includes the owner's later surface refinement (14 layers, deeper relief,
roughness variation and restrained steel rust). Generate into **artworks/CORE-07-Refined** to
preserve this original 11-layer project. See [the refined guide](../CORE-07-Refined/README.md).

The small creator lives in `source/` as an independent Cargo package; it does not modify the
application crates. Keep this directory at `artworks/CORE-07` inside a SubCraft checkout:

```powershell
$env:CARGO_TARGET_DIR = (Join-Path (Get-Location) 'target')
Copy-Item Cargo.lock artworks/CORE-07/source/Cargo.lock
cargo test --release --manifest-path artworks/CORE-07/source/Cargo.toml
cargo run --release --manifest-path artworks/CORE-07/source/Cargo.toml -- artworks/CORE-07-Refined
```

This rebuilds the artwork, so make a copy before experimenting. Generation uses real app
commands and performs a full save/reopen comparison of both the editable document and every
composite texel. The five creator tests check UV islands, material-mask landmarks, finite
beveled mesh/tangent frames, relief/cage bounds and localized weathering. Numerical generation checks also require finite material channels,
non-flat baked normal details and near-unit decoded normal vectors.

The native desktop preview is included as `renders/CORE-07-in-app.png` in the asset bundle
(also `docs/screenshots/core-07-native.png` in the SubCraft checkout).
Interactive pointer painting and installed-app update workflows are outside
this artwork test. Source maps and exports are not hand-retouched outside SubCraft.
