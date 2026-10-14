# Getting started with SubCraft

The welcome hub and in-app guide are in development for the next desktop release. In v0.1.3,
use **File → New from mesh…** or **File → New (sample crate)** to follow these steps.

Start with **Try the sample** on the welcome screen, or bring a model with UVs. Open
**Help → Quick start** for the same six steps inside the app. You can move the guide aside
while working, use Back/Next, or close it and reopen it later.

<img src="screenshots/welcome.png" alt="Welcome screen with model/project actions, desktop recent files and startup preference" width="760">

## 1. Open a model

Choose **New from a model…** on the welcome screen or **File → New from mesh…**. Desktop accepts
OBJ, GLB/glTF and FBX. The browser accepts OBJ and GLB/glTF; upload a `.gltf` together with its
`.bin` and any files it needs. A GLB is easier to share because those resources can be embedded.

Your model needs UVs for painting and texture export. Separate material IDs become texture sets;
switch sets in the **Texture Sets** panel. Imported object names appear in **Parts**.

To resume work, use **Open project…** for a `.subcraft` file. Desktop recent files list both
models and projects, and mark moved/deleted files as missing. Remove an entry to tidy the list.
Turn off **Show this screen on startup** here or in **Edit → Preferences** if you prefer.

## 2. Bake mesh maps

Click **Bake** in the top bar (Ctrl+B). Start with curvature and ambient occlusion at 512 pixels;
increase resolution and sample count once the material looks right. Wear and dirt masks use
these maps. A normal bake from a high-poly model also needs a low-poly model with UVs.

Inspect the results in **2D View**, using its map selector. Baking saves the maps in your project.

<img src="screenshots/baked-ao-2d-view.png" alt="Baked ambient occlusion inspected in the 2D view" width="760">

## 3. Drag a smart material

In **Shelf → Smart Materials**, drag **Worn Painted Metal** onto the model or a layer. Click a
layer in its folder and adjust its fill or generator in **Properties**. Undo with Ctrl+Z to try
another material. Each texture set has its own layer stack.

<img src="screenshots/smart-material-worn-metal.png" alt="Worn Painted Metal applied to the sample crate" width="760">

## 4. Paint and use Parts

Add/select a paint layer and use **Brush** (B) to paint with the left mouse button. Right-click
the piece under the cursor for **Fill**, **Paint only here**, layer masks and generator masks.
Switch the part type in **Parts** to work with connected pieces, UV islands, objects or material
IDs. Part-limited painting prevents brush writes outside that selection.

Hover a piece and press **I** to isolate it or **H** to hide it. **Esc** restores visibility;
hiding pieces changes the view, while your saved model and exported textures keep all parts.
Right-drag orbits, the wheel zooms, and **Home** resets the camera.

<img src="screenshots/quick-start-parts.png" alt="Step four of the in-app guide beside the Parts panel and painted crate" width="760">

## 5. Save your project

Use **File → Save** (Ctrl+S) to save a `.subcraft` file containing the model, layers, imported
images and baked maps. Desktop saves to the chosen location. The browser downloads the project;
keep that download so you can reopen it next time.

Desktop autosave creates recovery copies in SubCraft's data folder. It does not overwrite your
project file. Save your own project regularly; in the browser, download a backup when prompted.

## 6. Export textures

Click **Export** (Ctrl+Shift+E). Choose PBR metal/rough, Unreal, Unity HDRP or glTF to match your
target application. Desktop writes to your chosen folder; the browser downloads files and may
ask you to allow multiple downloads. Check the exported maps in the app or engine using them.

The Unreal template packs ORM and uses DirectX normals; PBR/glTF use OpenGL normals. Choose the
right template before judging the result under lighting.

For more detail, see the [README](../README.md), [changelog](../CHANGELOG.md) and
[development notes](development.md).
