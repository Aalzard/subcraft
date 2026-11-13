# Export to Blender (round trip)

**File > Export to Blender…** (also under **File > Send to**) sends the model to Blender and
brings a sculpt back as the high poly for baking. It is optional and works offline: SubCraft only
writes files and starts the Blender already on your computer.

## Main use: sculpt detail on a game low poly, then bake it

1. Open the low poly in SubCraft (for example an AI-generated game model).
2. **File > Export to Blender…**, untick **Include textures** (mesh only), click **Export and open Blender**.
3. In Blender: add a **Multires** modifier, subdivide, sculpt. Don't move, rotate or scale the object.
4. Open the **SubCraft** tab in the 3D view's sidebar (press **N**) and click **Send to SubCraft**.
5. SubCraft opens the **Bake** window with your sculpt already set as the high poly. Bake.

Your SubCraft low poly and its UVs are never changed, so the bake lines up. You can send the
sculpt again as often as you like; each send replaces the previous one.

## Looking at the textured model in Blender

Leave **Include textures** on. The textures are exported with the chosen preset (default:
*Blender bridge*: base colour, metallic, roughness, OpenGL normal, plus emissive, opacity and AO
when the project has them) and wired into a Principled BSDF:

* base colour and emissive as sRGB, every other map as Non-Color;
* normal through a Normal Map node (DirectX normals of other presets are flipped to OpenGL);
* packed maps (ORM, mask maps) split into their channels, smoothness / glossiness inverted into
  roughness, opacity into Alpha;
* AO is loaded but not connected (the Principled BSDF has no AO input; Blender computes its own).

## Where things go

* Exchange folder: next to a saved project (`<project>_blender/`), otherwise in the system temp
  folder (`subcraft-blender/<mesh>`). It holds the low poly (`*_lowpoly.obj`), `textures/`,
  `bridge.json`, `subcraft_bridge.py`, Blender's console output (`blender_log.txt`) and the sculpt
  coming back (`from_blender/sculpt_high.obj`).
* The low poly is written as one welded object (UV seams don't split it), so it can be sculpted
  without tearing.
* Blender opens on an empty scene with just your model (objects of your startup file are left out;
  a saved .blend is never touched) and skips the splash screen this one time.
* If you restart SubCraft, the link is gone: pick `from_blender/sculpt_high.obj` in the Bake
  window's High poly field yourself.

## Finding Blender

SubCraft uses, in order: the path in **Settings > General > Blender** (or typed in the Export to
Blender window; a folder works too), the `SUBCRAFT_BLENDER` environment variable, `blender` on
`PATH`, Blender shortcuts in the Windows Start menu (so an install in any folder is found), then
the usual install folders (Windows: Program Files\Blender Foundation, Steam,
%LOCALAPPDATA%\Programs; Linux: snap, flatpak, `~/blender-*`, `/opt/blender*`; macOS:
/Applications). The newest version found wins. If none is found the window says so and the
export button stays off. Blender 3.6 or newer is needed.
