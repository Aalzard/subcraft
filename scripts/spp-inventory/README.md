Research scripts behind `docs/spp-procedurals.md` (not part of the build). Python 3, `h5py`, `py7zr`, Pillow.

1. Extract each project's `paint/document.bin` to `docs/<name>.bin` and its `resources/*.sbsar` to `sbsar/<sha1>.sbsar` (h5py).
2. `classes.py` counts object classes; `graphs.py` tallies procedural graphs (needs `sbinfo.json` from `sbsar.py`
   and `docs/*.bin`); `sbsar.py` unpacks the 7z archives and summarises XML and `.sbsasm`.
`docmod.py` is the schema-driven parser of `document.bin` (same format as `crates/mesh/src/spp/document.rs`).
