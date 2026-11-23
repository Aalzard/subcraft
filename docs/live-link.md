# Live link to Unity, Unreal and Godot

**File > Live link...** keeps a game engine project's copy of your textures up to date while you
paint.

1. Click **Choose engine project...** and pick the engine project's folder (or any folder inside
   it). SubCraft recognises Godot (`project.godot`), Unity (`Assets` + `ProjectSettings`) and
   Unreal (`*.uproject`). The choice is saved with the SubCraft project.
2. Tick **Send automatically after edits**, or press **Send now**.

About a second after you stop painting, SubCraft exports the textures in the background into:

| Engine | Folder | Maps |
| --- | --- | --- |
| Godot 4 | `subcraft/<project>/` | Base Color, Normal (OpenGL), ORM, Emissive, Height |
| Unity | `Assets/SubCraft/<project>/` | URP Lit: Base Map, Metallic + Smoothness, Normal |
| Unreal Engine | `Content/SubCraft/<project>/` | Base Color, Normal (DirectX), packed Occlusion / Roughness / Metallic |

The first send also writes the model as OBJ. Godot also gets one StandardMaterial3D (`.tres`) per
texture set, already wired to the textures: drag it onto the mesh. In Unity and Unreal, make a
material once and assign the textures; later sends replace the files and the engine reimports them
(Unity and Godot when their window gets focus, Unreal through Auto Reimport, on by default).

Headless, `subcraft-app`'s example does one send: `cargo run --release -p subcraft-app --example
live_link_send -- <engine project folder>`.
