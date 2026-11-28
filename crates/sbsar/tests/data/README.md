# Test archive

`CardBoard.sbsar` is copied unchanged from Adobe's
[USD-Fileformat-plugins](https://github.com/adobe/USD-Fileformat-plugins) repository
(`test/assets/sbsar/CardBoard.sbsar`, commit `6441b97807a59472337f9c3d02df089d45ed1c48`),
which is licensed under the Apache License 2.0. It is used only by the tests of this crate.

`EdgeGenerator.sbsar` is a tiny Substance generator made for these tests with
[open-sbsar](https://github.com/wtfmydarling/open-sbsar)'s `tools/write.py` (MIT / Apache 2.0) by
`make_edge_generator.py`: image inputs `curvature` and `ambient_occlusion`, one grayscale output
`mask` (curvature levels, combined with the ambient occlusion).

`InvertFilter.sbsar` is a tiny Substance filter made the same way by `make_invert_filter.py`:
image inputs `basecolor` and `roughness`, outputs `basecolor` (usage `baseColor`) and `roughness`,
each the input inverted.
