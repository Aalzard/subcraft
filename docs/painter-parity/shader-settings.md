# Painter parity: Shader Settings

Sources:
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/shader-settings/shader-settings
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/shader-settings/updating-a-shader
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/texture-set/texture-set-list
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/texture-set/texture-set-settings
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/subsurface-scattering/subsurface-scattering
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/subsurface-scattering/enabling-subsurface-in-a-project
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/subsurface-scattering/subsurface-parameters
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/subsurface-scattering/subsurface-material-type
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/custom-shaders
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/dynamic-material-layering
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/iray-renderer/viewer-and-mdl-settings
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/scripting-and-development/api-reference/shader-api/shader-api
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/scripting-and-development/api-reference/shader-api/changelog-shader-api
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/scripting-and-development/api-reference/shader-api/parameters-shader-api/all-custom-params-shader-api
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/scripting-and-development/api-reference/shader-api/parameters-shader-api/all-rendering-states-params-shader-api
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/scripting-and-development/api-reference/shader-api/shaders-shader-api/pbr-metal-rough-shader-api
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/scripting-and-development/api-reference/shader-api/shaders-shader-api/pbr-material-layering-shader-api
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/scripting-and-development/api-reference/shader-api/shaders-shader-api/toon-shader-api
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/scripting-and-development/api-reference/shader-api/shaders-shader-api/pixelated-shader-api
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/scripting-and-development/api-reference/shader-api/libraries-shader-api/lib-pbr-shader-api (and the lib-alpha, lib-emissive, lib-pom, lib-sampler, lib-sss pages beside it, for parameter labels, defaults and ranges)
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/version-12-1
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/version-11-1
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/version-9-1
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-7-2
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/old-versions/version-2018-3
- https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/release-notes/all-changes

Checked against: origin/main 297dcb3 (2026-11-11)

General note on status: SubCraft has no Shader Settings panel and no shader abstraction. One fixed metal/roughness PBR shader
(`crates/render/src/viewport.wgsl`, mirrored by `crates/render/src/pathtrace.wgsl`) reads Base Color, Roughness, Metallic,
Normal, Height, Ambient Occlusion and Emissive; the only user-tunable shading value is View > "Height bump"
(`crates/ui/src/app.rs:1273`). The Opacity, Specular and User channels exist in the document (`crates/core/src/channel.rs:11`)
but are not shaded. Coverage gap in the source docs: the "Default Shaders" list on the Custom Shaders page does not render,
and the parameter list of the ASM and OpenPBR shaders is not documented anywhere in the help; those rows are marked
(unverified).

## Shader Settings window: undo stack and shader file

Layout notes: top strip with Undo and Redo arrows (a history separate from the main one), then a wide button showing the
current shader file (opens a mini-shelf of shaders), the instance name field and a Restore defaults button. An "Outdated"
tag is shown beside the shader button when the project's copy is older than the library's. Below come the parameter groups
of the loaded shader, then the Displacement and Tessellation groups. The window follows the selected Texture Set.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Shader Settings window | Dock toolbar (right) / Window menu | docked icon | done: Window > Shader settings, or Shader settings… in a texture set's shader menu (optional dock tab) | crates/ui/src/panels/shader_settings.rs, crates/ui/src/app.rs (Tab::ShaderSettings) | 207 |
| Window follows the selected Texture Set's shader instance | Shader Settings | - | done | crates/ui/src/panels/shader_settings.rs (show) | 207 |
| Undo (shader-only history) | Shader Settings, top | - | missing | - | - |
| Redo (shader-only history) | Shader Settings, top | - | missing | - | - |
| Shader file button (opens mini-shelf to pick a shader) | Shader Settings, top | OpenPBR shader for new projects without a template (12.1); ASM before | missing | - | - |
| Instance name | Shader Settings, top | "Main shader" (unverified) | done: editable; renamed when the field is left | crates/ui/src/panels/shader_settings.rs | 207 |
| Restore defaults | Shader Settings, top | - | done | crates/app/src/shader_instances.rs (restore_shader_defaults) | 207 |
| "Outdated" marker on the shader file | Shader Settings, top | - | missing | - | - |
| Update a shader by re-picking it in the mini-shelf | Shader Settings, shader button | - | missing | - | - |
| Update a shader from the Resource Updater's Shader tab | Plugins toolbar > Resource Updater | - | missing | - | - |
| Reloading a shader keeps its parameter values | Shader Settings | - | missing | - | - |
| Tooltips on shader parameters | Shader Settings, parameter rows | - | missing | - | - |

## Shader instances

Layout notes: managed from the Texture Set list (a shader button at the right of each Texture Set row) and the Texture Set
settings (a Shader instance dropdown). All Texture Sets share one instance by default.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Shader button per Texture Set (shows the instance, opens the instance menu) | Texture Set list, right of each row | Main shader | done | crates/ui/src/panels/texture_sets.rs (set_rows) | 207 |
| New shader instance | Texture Set list > shader button menu | - | done: a copy of the set's current instance, assigned to it | crates/app/src/shader_instances.rs (new_shader_instance) | 207 |
| Assign an existing instance to the Texture Set | Texture Set list > shader button menu | shared instance | done: several sets can share one | crates/app/src/shader_instances.rs (assign_shader_instance) | 207 |
| Shader instance (dropdown) | Texture Set settings | shared instance | done: Texture Set Settings > Shader Instance | crates/ui/src/panels/texture_sets.rs (general) | 207 |
| Rename instance | Shader Settings > Instance name | - | done | crates/app/src/shader_instances.rs (rename_shader_instance) | 207 |
| Instance names kept unique | Shader Settings (12.1) | - | done: " (2)", " (3)"... appended | crates/doc/src/shader.rs (unique_name) | 207 |
| Undo of instance creation | Shader Settings undo | - | missing | - | - |
| Import Shader Parameters (JSON file) | Texture Set list > settings menu | - | missing | - | - |
| Shader taken from the project template on USD / glTF import | New project (12.1) | - | missing | - | - |
| Default shader for thumbnail generation | Preferences | ASM (7.2 notes) | missing | - | - |

## Shaders shipped with Painter

Layout notes: shown as tiles in the shader mini-shelf and in the Assets window's shader category. The Default / range
column lists the channels each shader reads (its channel requirements); "common" means Height, Normal, Ambient occlusion,
Emissive plus the baked Normal and AO mesh maps, which the shared libraries bind for every PBR shader. Names without a doc
page are marked (unverified).

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| OpenPBR shader (default since 12.1) | Shader mini-shelf | channels: OpenPBR 1.1 set (base, specular, coat, fuzz, transmission, subsurface, thin-film, emission, geometry; exact list not documented) | missing | - | - |
| Adobe Standard Material (asm-metal-rough) | Shader mini-shelf | channels: Base color, Roughness, Metallic, Specular level, Specular edge color, Anisotropy level, Anisotropy angle, Coat opacity, Coat color, Coat roughness, Coat specular level, Coat normal, Sheen opacity, Sheen color, Sheen roughness, Scattering, Scattering color, Opacity, Translucency, Absorption color, common (mapping partly unverified) | missing | - | - |
| pbr-metal-rough | Shader mini-shelf | channels: Base color, Roughness, Metallic, Specular level, Scattering, Displacement, common | partial: the built-in fixed shader; no Specular level, Scattering or Displacement, no parameters | crates/render/src/viewport.wgsl:1 | - |
| pbr-metal-rough-with-alpha-blending | Shader mini-shelf | channels: as pbr-metal-rough plus Opacity (unverified) | partial: Texture Set Settings > Alpha = Alpha blending on the one built-in shader (no shader instances yet); blended after the opaque sets without depth write, unsorted | crates/doc/src/texture_set.rs (AlphaSettings), crates/render/src/viewport.wgsl (surface_alpha) | #145 |
| pbr-metal-rough-with-alpha-test (docs also write pbr-metal-rough-alpha-test; draws back faces) | Shader mini-shelf | channels: as pbr-metal-rough plus Opacity | partial: Texture Set Settings > Alpha = Alpha test (back faces were already drawn) | crates/doc/src/texture_set.rs (AlphaSettings), crates/render/src/viewport.wgsl (surface_alpha) | #145 |
| pbr-metal-rough-anisotropy-angle | Shader mini-shelf | channels: as pbr-metal-rough plus Anisotropy level, Anisotropy angle | missing | - | - |
| pbr-coated (clear coat) | Shader mini-shelf | channels: as pbr-metal-rough plus User0 (coat roughness), User1 (coat mask) | missing | - | - |
| pbr-spec-gloss | Shader mini-shelf | channels: Diffuse, Specular, Glossiness, Scattering, common (unverified) | missing | - | - |
| pbr-spec-gloss-with-alpha-blending | Shader mini-shelf | channels: as pbr-spec-gloss plus Opacity (unverified) | missing | - | - |
| pbr-spec-gloss-with-alpha-test | Shader mini-shelf | channels: as pbr-spec-gloss plus Opacity (unverified) | missing | - | - |
| non-pbr-spec-gloss | Shader mini-shelf | channels: Diffuse, Specular, Glossiness (unverified) | missing | - | - |
| dota-2 | Shader mini-shelf | channels: not documented | missing | - | - |
| pbr-material-layering | Shader mini-shelf | channels: 4 material sub-stacks (Base color, Roughness, Metallic, Normal each) and 3 Blending mask stacks | missing | - | - |
| pbr-velvet | Shader mini-shelf | channels: not documented (unverified) | missing | - | - |
| toon | Shader mini-shelf | channels: Base color; Curvature mesh map | missing | - | - |
| pixelated | Shader mini-shelf | channels: Base color | missing | - | - |
| surface-shader (minimal API sample) | Shader API reference | channels: none | missing | - | - |
| Unused channels are skipped by the shader (optimisation) | All shaders (7.2) | automatic | missing | - | - |

## Default PBR shader parameters: Common Parameters

Layout notes: these are the groups and labels declared by the libraries the pbr-metal-rough shader imports; the ASM shader
shows the same labels inside its own groups. Group order in the panel: Common Parameters, Parallax Occlusion Mapping,
Subsurface Scattering Parameters. Alpha threshold and Alpha dithering only appear in shaders that use opacity.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| AO Intensity | Shader Settings > Common Parameters | 0.75; 0-1 | done: 0-1, default 0.75; blends the AO channel toward no occlusion | crates/render/src/viewport.wgsl (fs_main) | 207 |
| Quality (specular samples) | Shader Settings > Common Parameters | Low (16 spp); options Very low (4), Low (16), Medium (32), High (64), Very high (128), Ultra (256) | missing: the raster viewport uses prefiltered environment lookups, not sample counts | - | - |
| Horizon Fading | Shader Settings > Common Parameters | 1.3; 0-2 | done: 0-2, default 1.3; fades reflections a normal map tilts below the surface | crates/render/src/viewport.wgsl (fs_main) | 207 |
| Emissive Intensity | Shader Settings > Common Parameters | 1; 0-100 | done: 0-100, default 1; also scales the emission glow and glare | crates/render/src/viewport.wgsl (fs_main), fs_emissive | 207 |
| Alpha threshold | Shader Settings > Common Parameters (alpha-test shaders) | 0.33; 0-1 | done (per texture set until shader instances exist) | crates/doc/src/texture_set.rs (AlphaSettings), crates/render/src/viewport.wgsl (surface_alpha) | #145 |
| Alpha dithering | Shader Settings > Common Parameters (alpha shaders) | off | done: 4x4 Bayer, per texture set | crates/doc/src/texture_set.rs (AlphaSettings), crates/render/src/viewport.wgsl (surface_alpha) | #145 |
| Height force (removed from the default shaders in 2018.1) | Shader Settings (legacy) | not documented | partial: View > "Height bump" 0-0.1 (default 0.02), global rather than per instance | crates/ui/src/app.rs:1273 | - |

## Default PBR shader parameters: Parallax Occlusion Mapping

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Enable | Shader Settings > Parallax Occlusion Mapping | off | missing | - | - |
| Strength | Shader Settings > Parallax Occlusion Mapping | 1; 0.01-10 | missing | - | - |
| Minimum samples | Shader Settings > Parallax Occlusion Mapping | 4; 1-64 | missing | - | - |
| Maximum samples | Shader Settings > Parallax Occlusion Mapping | 16; 1-64 | missing | - | - |

## Default PBR shader parameters: Subsurface Scattering Parameters

Layout notes: needs a Scattering channel in the Texture Set and the project-wide switch in Display Settings; see the
Subsurface scattering section below.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Enable | Shader Settings > Subsurface Scattering Parameters | on | missing | - | - |
| Scattering Type | Shader Settings > Subsurface Scattering Parameters | Skin; options Translucent, Skin (pbr shaders), plus Red Shift/Rayleigh (ASM) | missing | - | - |
| Scale | Shader Settings > Subsurface Scattering Parameters | 0.5; 0.01-1 | missing | - | - |
| Color | Shader Settings > Subsurface Scattering Parameters | RGB 0.701, 0.301, 0.305 | missing | - | - |

## ASM / OpenPBR shader parameter groups

Layout notes: the ASM shader (default from 7.2 to 12.0) and the OpenPBR shader (default since 12.1) regroup the parameters
to match other Substance 3D apps. The help only names the groups and switches quoted in the 7.2 and 9.1 release notes;
everything else here is from memory and marked (unverified). No per-parameter defaults or ranges are documented.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Geometry > Opacity: Alpha test | Shader Settings (ASM, 9.1) | off | partial: same switch in Texture Set Settings (no ASM shader) | crates/doc/src/texture_set.rs (AlphaSettings), crates/render/src/viewport.wgsl (surface_alpha) | #145 |
| Geometry > Opacity: Alpha blending | Shader Settings (ASM, 9.1) | off | partial: same switch in Texture Set Settings (no ASM shader) | crates/doc/src/texture_set.rs (AlphaSettings), crates/render/src/viewport.wgsl (surface_alpha) | #145 |
| Geometry > Opacity: Alpha threshold | Shader Settings (ASM) | 0.33; 0-1 (from lib-alpha) | missing | - | - |
| Geometry > Opacity: Alpha dithering | Shader Settings (ASM, 9.1) | off | missing | - | - |
| Interior: Translucency (enable; needs a Translucency channel) | Shader Settings (ASM, 9.1) | off | missing | - | - |
| Interior: Absorption (enable; uses the Absorption color channel) | Shader Settings (ASM, 9.1) | off | missing | - | - |
| Interior: Subsurface scattering (Enable, Type, Scale, Color) | Shader Settings (ASM) | see SSS group above | missing | - | - |
| Specular: Anisotropy (enable) | Shader Settings (ASM, 7.2) | (unverified) | missing | - | - |
| Specular: Specular edge color (enable) | Shader Settings (ASM, 7.2) | (unverified) | missing | - | - |
| Clear coat (enable) | Shader Settings (ASM, 7.2) | (unverified) | missing | - | - |
| Sheen (enable) | Shader Settings (ASM, 7.2) | (unverified) | missing | - | - |
| Bent normals shading | Shader Settings / shader API | (unverified) | missing | - | - |
| Parameters shown or hidden by a "visible if" condition | Shader Settings (7.2) | - | missing | - | - |

## Other shipped shaders: documented parameters

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| pbr-coated: surface behaviour | Shader Settings | options Keep Normal Details, Smooth surface | missing | - | - |
| pbr-coated: secondary (coat) roughness driven by User0 | Texture Set channel | - | missing | - | - |
| pbr-coated: coat mask driven by User1 | Texture Set channel | - | missing | - | - |
| toon / pixelated: Unlit outline thickness | Shader Settings | 0.4; 0-1 | missing | - | - |
| toon / pixelated: Lit outline thickness | Shader Settings | 0.1; 0-1 | missing | - | - |
| toon: Use curvature | Shader Settings | off | missing | - | - |
| pbr-material-layering: Debug Mode | Shader Settings | off | missing | - | - |
| pbr-material-layering: Debug channel | Shader Settings | BaseColor; options BaseColor, Roughness, and further channels | missing | - | - |
| pbr-material-layering: Normal from Masks | Shader Settings | off | missing | - | - |
| pbr-material-layering: Material 1 coords | Shader Settings > Material 1 | 5; 0.01-128 | missing | - | - |
| pbr-material-layering: Material 2 coords | Shader Settings > Material 2 | 5; 0.01-128 | missing | - | - |
| pbr-material-layering: Material 3 coords | Shader Settings > Material 3 | 5; 0.01-128 | missing | - | - |
| pbr-material-layering: Material 4 coords | Shader Settings > Material 4 | 5; 0.01-128 | missing | - | - |
| pbr-material-layering: Normal Intensity 1 | Shader Settings > Material 1 | 1; 0-1 | missing | - | - |
| pbr-material-layering: Normal Intensity 2 | Shader Settings > Material 2 | 1; 0-1 | missing | - | - |
| pbr-material-layering: Normal Intensity 3 | Shader Settings > Material 3 | 1; 0-1 | missing | - | - |
| pbr-material-layering: Normal Intensity 4 | Shader Settings > Material 4 | 1; 0-1 | missing | - | - |
| pbr-material-layering: Normal from Mask Intensity 2 | Shader Settings > Material 2 | 0; -10 to 10 | missing | - | - |
| pbr-material-layering: Normal from Mask Intensity 3 | Shader Settings > Material 3 | 0; -10 to 10 | missing | - | - |
| pbr-material-layering: Normal from Mask Intensity 4 | Shader Settings > Material 4 | 0; -10 to 10 | missing | - | - |
| pbr-material-layering: Normal from Mask 1 Offset | Shader Settings > Material 2 | 0.1; 0-1 | missing | - | - |
| pbr-material-layering: Normal from Mask 2 Offset | Shader Settings > Material 3 | 0.1; 0-1 | missing | - | - |
| pbr-material-layering: Normal from Mask 3 Offset | Shader Settings > Material 4 | 0.1; 0-1 | missing | - | - |
| pbr-material-layering: material slots (bind a material resource per sub-stack) | Shader Settings | - | missing | - | - |

## Displacement and tessellation

Layout notes: two groups at the bottom of the Shader Settings window, Displacement first, then Tessellation (the help spells
it "Tesselation"). Scene and Physical size units need a mesh with real-world units.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Source Channel | Shader Settings > Displacement | Height; options Height, Displacement | missing: height only perturbs the shading normal | - | - |
| Scale unit | Shader Settings > Displacement (11.1) | Normalized; options Normalized, Scene, Physical size (cm) | missing | - | - |
| Scale amount | Shader Settings > Displacement | not documented | missing | - | - |
| Subdivision Mode | Shader Settings > Tessellation | Uniform; options Uniform, Edge Length | missing | - | - |
| Subdivision Count (Uniform mode) | Shader Settings > Tessellation | not documented; 1-32 | missing | - | - |
| Max Length (Edge Length mode) | Shader Settings > Tessellation | not documented; 1/value of the scene size | missing | - | - |
| Displacement channel in the Texture Set | Texture Set settings > channels | not present by default | missing | - | - |
| Displacement shown in the Iray mode | Rendering mode | - | missing | - | - |

## Subsurface scattering feature

Layout notes: three switches must all be on: a Scattering channel in the Texture Set (grayscale mask, black by default), the
project-wide switch in Display Settings, and Enable in the shader's SSS group. The real-time effect is a screen-space blur;
Iray renders it through the MDL. The material-type rows are the starting values the help suggests.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Scattering channel | Texture Set settings > add channel | absent; black when added | missing | - | - |
| Scattering color channel (ASM) | Texture Set settings > add channel | absent | missing | - | - |
| Project-wide subsurface switch and Sample Count | Display Settings | off | missing | - | - |
| Real-time screen-space SSS in the viewport | 3D view | - | missing | - | - |
| SSS in the Iray mode (skin / SSS MDL) | Rendering mode | - | missing | - | - |
| Shaders with SSS support: pbr-metal-rough, pbr-metal-rough-alpha-test, pbr-coated, pbr-spec-gloss, ASM | Shader mini-shelf | - | missing | - | - |
| Scattering color visible in the 2D view without SSS support | 2D view | - | missing | - | - |
| Suggested values: Human skin | Shader Settings > SSS | Scale 0.05-0.2 (printed as '05 to 0.2'); Color 0.575, 0.894, 0.376 | missing | - | - |
| Suggested values: Jade | Shader Settings > SSS | Scale 25 as printed (likely 0.25); Color 0.575, 0.894, 0.376 | missing | - | - |
| Suggested values: Marble | Shader Settings > SSS | Scale 25 as printed (likely 0.25); Color 0.845, 0.777, 0.525 | missing | - | - |
| Suggested values: Wax | Shader Settings > SSS | Scale 6 or more as printed (likely 0.6); Color 0.820, 0.470, 0.205 | missing | - | - |
| Suggested values: Milk | Shader Settings > SSS | Scale 7 as printed (likely 0.7); Color 0.895, 0.833, 0.750 | missing | - | - |

## Custom shaders (GLSL shader API)

Layout notes: a custom shader is a .glsl file holding only the surface part of the fragment shader; it is added by dropping
it into the shader category of the Assets window or a library's shaders folder. The rows below are feature groups, not
individual API functions.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Load a custom .glsl surface shader (drop into the shader shelf / import as shader resource) | Assets window | - | missing | - | - |
| Surface shader entry point (`shade(V2F)`) with outputs for albedo, diffuse and specular shading, emissive, alpha, SSS coefficients | Shader API | - | missing | - | - |
| Engine-provided parameters (`param auto`: document channels, mesh maps, environment map/rotation/exposure, camera and screen data, shadow mask, blue noise, UV tile data) | Shader API | - | missing | - | - |
| Rendering state: back-face culling on/off | Shader API | on | missing | - | - |
| Rendering state: blending (none, over, over_premult, add, multiply) | Shader API | none | missing | - | - |
| Rendering state: non-local sampling | Shader API | off | missing | - | - |
| Custom parameter widgets: color (RGB / RGBA) | Shader API > Shader Settings | per shader | missing | - | - |
| Custom parameter widgets: spinbox (int and float, 1 to 4 components) | Shader API > Shader Settings | per shader | missing | - | - |
| Custom parameter widgets: slider (min, max, optional step) | Shader API > Shader Settings | per shader | missing | - | - |
| Custom parameter widgets: checkbox (bool) | Shader API > Shader Settings | per shader | missing | - | - |
| Custom parameter widgets: texture slot (usage texture or environment, default color) | Shader API > Shader Settings | per shader | missing | - | - |
| Custom parameter widgets: combobox (named integer values) | Shader API > Shader Settings | per shader | missing | - | - |
| Parameter metadata: label, group, description (tooltip), visible | Shader API | - | missing | - | - |
| Embedded GLSL libraries (lib-alpha, bayer, defines, emissive, env, normal, pbr, pbr-aniso, pom, random, sampler, sparse, sss, utils, vectors) | Shader API | - | missing | - | - |
| Metadata: custom-ui (replace the parameter UI with a QML view) | Shader API | - | missing | - | - |
| Metadata: mdl (MDL material used for the shader in the Iray mode) | Shader API | - | missing | - | - |
| Dynamic material layering: declare extra editable stacks from the shader | Shader API | - | missing | - | - |
| Dynamic material layering: bind materials as shader parameters | Shader API | - | missing | - | - |
| Mesh vertex colors readable in a shader | Shader API | - | missing | - | - |
| Multiple texture coordinate sets exposed | Shader API | - | missing | - | - |
| Overridable gamma / tone-mapping helpers | Shader API | - | missing | - | - |
| Example shaders (pixelated, toon, pbr-metal-rough) | Shader API reference | - | missing | - | - |
| Shader API documentation and changelog | Help > Documentation > Shader API | - | missing | - | - |
| Updating a custom shader after an API change | Shader Settings / Resource Updater | - | missing | - | - |
