# SubCraft pack 2: lines, sci-fi, and mechanical surfaces

40 new original procedural PNGs: 20 seamlessly repeating OpenGL RGB normal
maps and 20 grayscale brush alphas. Delivered at 1024 x 1024, all 8-bit.
Everything is constructed from math, seeded randomness, and procedural drawing;
there are no external image inputs. Intended for your CC0 release.

## Generate

Python 3.10+, NumPy, Pillow, and SciPy:

```sh
python -m pip install numpy Pillow scipy
python generate_textures.py --pack scifi
python generate_textures.py --pack scifi --size 2048 --output output_2k
python generate_textures.py --pack scifi --size 4096 --output output_4k
```

`--pack scifi` writes new maps into `output/textures/` and `output/alphas/`,
and previews and reports into `output/pack2/`. Use `--output` to keep sizes
separate. `--pack starter` regenerates the original 20 assets. `--pack all`
is now the default and generates all 60, with combined previews in `output/`.
Existing generated files with the same name are replaced.

Normals use height fields, wrapped central differences, OpenGL green-up
encoding, and strength 1.0. Flat pixels are (128,128,255). Import them as
linear/non-color normal data. Alphas are single-channel L PNGs: white paints,
black does nothing. Their 3.5% perimeter margin is exactly black, and smooth
edges use deterministic sub-LSB dither to avoid visible 8-bit banding.

Seeds are fixed per texture. With the same arguments and library versions,
the texture PNGs reproduce byte-for-byte. The verified environment used
Python 3.12, NumPy 2.5.1, Pillow 12.3.0, and SciPy 1.18.1. Rasterized lines
and bevels retain their UV scale at larger sizes, with pixel-scale differences
in antialiasing. Larger resolutions take more time and memory.

## Inspect

- `preview_normals.png` and `preview_alphas.png`: the 40 new maps.
- `preview_seams.png` and `seam_previews/`: four tiled 2x2 previews.
- `CHECKS.md` and `validation.json`: per-map measurements and SHA-256 hashes.
- `run_verification.json`: repeatability and resolution checks.
- `QUALITY_NOTES.md`: visual review and possible refinements.

All 40 new maps were generated and checked at 1024, 2048, and 4096. A repeat
1024 run matched all 40 hashes, and the default all-pack run reproduced the
original 20 hashes as well. The delivered files are the 1024 versions.

The normals represent relief; vent holes and grate openings are recessed
height features. Use an opacity mask or geometry as well if you want real holes.

## Extend

Add a `my_texture(n)` function in `generate_textures.py` returning a finite
NumPy `(n,n)` float array: height for a normal, intensity for an alpha. Register
it in `SCIFI_NORMALS` or `SCIFI_ALPHAS`. The generator adds the filename suffix,
conversion, quantization, contact sheet entry, and numerical checks.

Use `grid`, `cell_grid`, periodic `noise`, wrapped deltas, or `tiled_drawing`
for normal recipes. Keep repeats integral and staggered row counts even.
Reserve a new fixed seed. Review the result visually as well as numerically;
the automatic seam measurements compare joined edge pixels against ordinary
internal adjacency distributions. Original alpha seeds remain stable when
switching between the three packs.
