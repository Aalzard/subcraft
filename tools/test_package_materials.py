#!/usr/bin/env python3
"""Checks of tools/package_materials.py on the repository's two fixture materials.

    python tools/test_package_materials.py

Exits non-zero on a failure. Needs Pillow, like the packaging script.
"""

import hashlib
import io
import json
import shutil
import sys
import tempfile
import zipfile
from pathlib import Path

from PIL import Image

sys.path.insert(0, str(Path(__file__).parent))
import package_materials as pm  # noqa: E402

FIXTURES = Path(__file__).resolve().parent.parent / "crates/assets/tests/fixtures/materials_pack"


def main():
    tmp = Path(tempfile.mkdtemp(prefix="subcraft-package-test-"))
    try:
        source = tmp / "source"
        shutil.copytree(FIXTURES, source)
        (source / "INDEX.md").write_text("# test pack\n", encoding="utf-8")
        (source / "_tools").mkdir()
        (source / "_tools" / "material.json").write_text("{}", encoding="utf-8")
        before = sorted(p.relative_to(source).as_posix() for p in source.rglob("*"))
        out = tmp / "out"
        results = pm.package(source, 3, out, sizes=("1k", "2k"), jobs=1)
        assert "2k" not in results and not (out / "subcraft-materials-v3-2k.zip").exists(), "no 2048px sources: no 2K zip"
        path, digest, count = results["1k"]
        assert path.name == "subcraft-materials-v3-1k.zip" and count == 2, (path, count)
        sha = (out / (path.name + ".sha256")).read_text(encoding="utf-8").split()
        assert sha == [hashlib.sha256(path.read_bytes()).hexdigest(), path.name] and sha[0] == digest
        with zipfile.ZipFile(path) as z:
            names = z.namelist()
            assert "INDEX.md" in names and not any(n.startswith("_tools") for n in names), names
            meta = json.loads(z.read("metal_test/material.json"))
            # The fixtures are 4 x 4: kept at their native size, never upscaled.
            assert meta["resolution"] == 4 and meta["resolution_note"] == "native 4px: no 1024px source", meta
            assert "emissive" not in meta["maps"], "the listed but missing emissive map is left out"
            for key, file in meta["maps"].items():
                im = Image.open(io.BytesIO(z.read(f"metal_test/{file}")))
                assert im.size == (4, 4), (file, im.size)
            assert "metal_test/preview.png" in names and "wood_test/preview.png" in names
        # The source is only read, and the same input gives the same zip.
        assert sorted(p.relative_to(source).as_posix() for p in source.rglob("*")) == before
        again = pm.package(source, 3, tmp / "again", sizes=("1k",), jobs=1)["1k"][1]
        assert again == digest, "deterministic zips"
        print("package_materials: all checks passed")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    main()
