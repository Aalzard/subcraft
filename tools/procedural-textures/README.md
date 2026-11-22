# Procedural texture generator

`generate_textures.py` draws tileable normal maps and brush alphas from math only
(numpy noise, Voronoi, shapes). It uses no image inputs and no AI image models.
`output/` holds the last generated set at 1024 px.

The app does not read this folder. The textures it ships live in `assets/starter-pack/`.

```
python -m pip install numpy Pillow scipy
cd tools/procedural-textures
python generate_textures.py --pack all --size 1024
```

See `output/README.md` for packs and options.
