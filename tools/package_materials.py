#!/usr/bin/env python3
"""Package a "SubCraft Materials" source folder into the release zips the app downloads.

    python tools/package_materials.py D:/SubCraft-SmartMaterials --version 1 --out dist

writes, for each size (1K and 2K by default):

    dist/subcraft-materials-v1-1k.zip   dist/subcraft-materials-v1-1k.zip.sha256
    dist/subcraft-materials-v1-2k.zip   dist/subcraft-materials-v1-2k.zip.sha256

Each zip's top level is the pack itself, the layout `crates/app/src/materials_download.rs`
unpacks and `crates/assets/src/materials_pack.rs` reads:

    INDEX.md
    <material>/material.json     (resolution updated; .scsmart and source image entries removed)
    <material>/preview.png
    <material>/<maps listed in material.json>, resized to the zip's size

The source folder is only read. See tools/package_materials.md.
"""

import argparse
import concurrent.futures
import hashlib
import io
import json
import os
import shutil
import sys
import zipfile
from pathlib import Path

from PIL import Image

SIZES = {"1k": 1024, "2k": 2048}
# Fixed timestamp: the same input gives the same zip bytes (and digest).
ZIP_TIME = (2026, 1, 1, 0, 0, 0)
# Entries of material.json that point at files the pack does not ship.
DROPPED_KEYS = ("smart_material", "source")


def zip_name(version, size_tag):
    return f"subcraft-materials-v{version}-{size_tag}.zip"


def material_folders(source, only=None):
    """Material sub-folders of the source: with material.json, not starting with _ or ."""
    out = []
    for p in sorted(Path(source).iterdir()):
        if not p.is_dir() or p.name.startswith(("_", ".")):
            continue
        if only and p.name not in only:
            continue
        if not (p / "material.json").is_file():
            print(f"skipped {p.name}: no material.json", file=sys.stderr)
            continue
        out.append(p)
    return out


def resized_png(path, size):
    """The image at `path` scaled to size x size (Lanczos), as PNG bytes, bit depth kept."""
    im = Image.open(path)
    im.load()
    if im.size != (size, size):
        if im.mode in ("I;16", "I;16B", "I;16L", "I"):
            # 16-bit greyscale (height): resample in 32-bit integers, then back to 16 bits.
            # (Converting to I;16 clips Lanczos overshoot to 0..65535; checked in the test.)
            im = im.convert("I").resize((size, size), Image.Resampling.LANCZOS).convert("I;16")
        elif im.mode in ("RGB", "RGBA", "L", "LA"):
            im = im.resize((size, size), Image.Resampling.LANCZOS)
        else:
            im = im.convert("RGBA" if "A" in im.mode else "RGB").resize((size, size), Image.Resampling.LANCZOS)
    buf = io.BytesIO()
    im.save(buf, format="PNG", compress_level=6)
    return buf.getvalue()


def resized_map(path, size):
    """Bytes of a resized map (PNG-ish formats); EXR is copied unresized when no EXR reader is
    installed (a warning says so). Returns (bytes, warning or None)."""
    if path.suffix.lower() == ".exr":
        try:
            import imageio.v3 as iio  # optional
            import numpy as np  # optional

            data = iio.imread(path)
            if data.shape[0] != size:
                ys = (np.arange(size) * data.shape[0] / size).astype(int)
                xs = (np.arange(size) * data.shape[1] / size).astype(int)
                data = data[ys][:, xs]
            return iio.imwrite("<bytes>", data, extension=".exr"), None
        except Exception as e:  # noqa: BLE001 - any missing piece means copy as is
            return path.read_bytes(), f"{path.name}: EXR copied unresized ({e})"
    return resized_png(path, size), None


def package_material(folder, size):
    """One material at one size: list of (zip entry name, bytes) and warnings."""
    folder = Path(folder)
    meta = json.loads((folder / "material.json").read_text(encoding="utf-8"))
    entries, warnings = [], []
    maps = meta.get("maps") or {}
    kept = {}
    for key, file in maps.items():
        src = folder / file
        if not src.is_file():
            warnings.append(f"{folder.name}: map {file} is missing, left out")
            continue
        try:
            with Image.open(src) as im:
                if src.suffix.lower() != ".exr" and max(im.size) < size:
                    warnings.append(f"{folder.name}: {file} is {im.size[0]}px, upscaled to {size}")
        except Exception:  # noqa: BLE001 - EXR or unreadable: resized_map reports it
            pass
        data, warning = resized_map(src, size)
        if warning:
            warnings.append(warning)
        entries.append((f"{folder.name}/{file}", data))
        kept[key] = file
    if not kept:
        return [], [f"{folder.name}: no map files, material skipped"]
    meta["maps"] = kept
    meta["resolution"] = size
    for k in DROPPED_KEYS:
        meta.pop(k, None)
    preview = meta.get("preview") or "preview.png"
    if (folder / preview).is_file():
        entries.append((f"{folder.name}/{preview}", (folder / preview).read_bytes()))
    else:
        warnings.append(f"{folder.name}: no preview")
    entries.append((f"{folder.name}/material.json", (json.dumps(meta, indent=2) + "\n").encode("utf-8")))
    return entries, warnings


def write_zip(path, entries):
    """Write entries (name, bytes) in name order with a fixed timestamp."""
    with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as z:
        for name, data in sorted(entries, key=lambda e: e[0]):
            info = zipfile.ZipInfo(name, date_time=ZIP_TIME)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o644 << 16
            z.writestr(info, data)


def write_sha256(zip_path):
    digest = hashlib.sha256(Path(zip_path).read_bytes()).hexdigest()
    Path(str(zip_path) + ".sha256").write_text(f"{digest}  {Path(zip_path).name}\n", encoding="utf-8")
    return digest


def package(source, version, out, sizes=("1k", "2k"), only=None, jobs=2):
    """Build every zip; returns {size tag: (zip path, digest, material count)}."""
    source, out = Path(source), Path(out)
    out.mkdir(parents=True, exist_ok=True)
    folders = material_folders(source, only)
    if not folders:
        raise SystemExit(f"no materials found in {source}")
    results = {}
    for tag in sizes:
        size = SIZES[tag]
        entries, count = [], 0
        index = source / "INDEX.md"
        if index.is_file():
            entries.append(("INDEX.md", index.read_bytes()))
        with concurrent.futures.ProcessPoolExecutor(max_workers=max(1, jobs)) as pool:
            for folder, (files, warnings) in zip(folders, pool.map(package_material, folders, [size] * len(folders))):
                for w in warnings:
                    print(f"[{tag}] {w}", file=sys.stderr)
                if files:
                    entries.extend(files)
                    count += 1
        path = out / zip_name(version, tag)
        write_zip(path, entries)
        digest = write_sha256(path)
        results[tag] = (path, digest, count)
        print(f"{path.name}: {count} materials, {path.stat().st_size / 1048576:.1f} MB, sha256 {digest}")
    return results


def main(argv=None):
    p = argparse.ArgumentParser(description="Package a SubCraft Materials source folder into release zips.")
    p.add_argument("source", help="the source folder (one sub-folder per material with material.json); only read")
    p.add_argument("--version", type=int, required=True, help="pack version N (release tag materials-vN)")
    p.add_argument("--out", default="dist", help="output folder (default: dist)")
    p.add_argument("--sizes", default="1k,2k", help="comma-separated sizes among 1k,2k (default: both)")
    p.add_argument("--only", default="", help="comma-separated material folder names (default: all)")
    p.add_argument("--jobs", type=int, default=2, help="worker processes (default: 2; the PC is busy)")
    a = p.parse_args(argv)
    if a.version < 1:
        p.error("--version must be 1 or more")
    sizes = [s.strip().lower() for s in a.sizes.split(",") if s.strip()]
    bad = [s for s in sizes if s not in SIZES]
    if bad:
        p.error(f"unknown size(s) {bad}; use 1k and/or 2k")
    if Path(a.out).resolve().is_relative_to(Path(a.source).resolve()):
        p.error("--out must not be inside the source folder (the source is only read)")
    only = {s.strip() for s in a.only.split(",") if s.strip()} or None
    package(a.source, a.version, a.out, sizes, only, a.jobs)


if __name__ == "__main__":
    main()
