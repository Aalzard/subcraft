# SubCraft procedural textures

60 original procedural PNG assets: 30 OpenGL tangent-space RGB normal maps and
30 single-channel grayscale brush alphas, all 1024 x 1024 and 8-bit. No source
photographs, downloaded images, copied textures, or image generation were used.
The assets are intended for your CC0 release.

## Run

Requires Python 3.10 or newer:

```sh
python -m pip install numpy Pillow scipy
python generate_textures.py
python generate_textures.py --pack starter
python generate_textures.py --pack scifi
python generate_textures.py --size 2048
python generate_textures.py --size 4096 --output output_4k
```

Run from the folder containing the script. The default output is `output/`.
The default pack is `all` (60 maps); `starter` selects the original 20,
and `scifi` selects the new 40. The new pack has its own contact sheets and
documentation in `output/pack2/`, generated when selecting `--pack scifi`.
Regeneration replaces generated files there. Use `--output` to keep another size.
For quick previews, `--size 256` and `--size 512` also work. Normal strength
defaults to 1.0; `--strength` adjusts it before slope limiting. Import normal
maps as linear/non-color data with OpenGL green-up convention; alphas use white
for paint, black for no paint.

Seeds are fixed separately for every texture. With the same arguments and
dependency versions, the texture PNGs reproduce byte-for-byte. The verified
environment used Python 3.12, NumPy 2.5.1, Pillow 12.3.0, SciPy 1.18.1. Random
features keep the same UV positions at larger sizes; antialiasing and dither
are rasterized at the requested resolution. Large sizes use more memory and time.

## Contents

- `textures/<Name>_Normal.png`: all thirty RGB maps.
- `alphas/<Name>_Alpha.png`: all thirty single-channel maps.
- `preview_normals.png`, `preview_alphas.png`: labeled contact sheets.
- `seam_previews/*_Normal_2x2.png`: four full-resolution tiled previews.
- `preview_seams.png`: the four 2x2 previews together.
- `CHECKS.md`, `validation.json`: measurements and texture SHA-256 fingerprints.
- `run_verification.json`: repeat-run and full 1024/2048/4096 checks.
- `QUALITY_NOTES.md`: visual review and possible improvements.
- `pack2/`: contact sheets, measurements, README, and quality notes for the
  twenty new normals and twenty new alphas with lines and sci-fi designs.

The generator asserts normal unit length within 8-bit quantization tolerance,
blue dominance, mean color, normal direction, exact flat encoding, alpha ranges,
black borders, and seam differences within ordinary internal scanline ranges.
It also compares wrapped conversion with conversion of a repeated 2x2 height
field. Smooth alpha ramps use deterministic sub-LSB dither to avoid visible
8-bit contour bands; no 16-bit exception is necessary.

Both packs were checked at 1024, 2048, and 4096. The updated generator's
default all-pack run reproduced all 60 texture PNGs at 1024. New-pack
resolution and repeatability results are in `pack2/run_verification.json`.
The supplied PNG set is 1024; generate larger sets using the commands above.

## Add a texture

1. Add a function `my_texture(n)` returning a finite NumPy `(n, n)` float array.
   For a normal map, return height; for an alpha, return paint intensity.
2. Give random operations a new fixed seed. Use `grid(n)` coordinates and the
   periodic `noise`, `fbm`, `wrap_delta`, or `mode='wrap'` filters for normals.
   Use integer pattern repeats and an even row count for staggered patterns.
   Never make a normal tile by painting a border over a nonperiodic image.
3. Register it as `'MyTexture': my_texture` in `SCIFI_NORMALS` or `SCIFI_ALPHAS`
   (or the `STARTER_` registries if adding it to the first pack).
   The registry adds the `_Normal.png` or `_Alpha.png` suffix automatically.
4. Run the script, read `CHECKS.md`, and inspect the contact sheet and a tiled
   preview. `height_to_normal` handles depth calibration and wrap derivatives;
   `quantize_alpha` handles intensity scaling, black margins, and dithering.

Do not add copyrighted source images: this pack is entirely procedural.
