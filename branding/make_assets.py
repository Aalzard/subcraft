"""Regenerate every SubCraft brand asset from the vector definition below.

Run from the repository root:  python branding/make_assets.py
Needs Pillow only. The geometry is the traced "Geometric Ribbon Star" mark on a 1254 x 1254 canvas;
the orange is the flat #FF7800 of the original artwork.
"""
from pathlib import Path
from PIL import Image

ORANGE = (255, 120, 0)
SIZE = 1254
SS = 4  # supersampling for clean anti-aliasing

# Two ribbon halves of the hexagonal S (straight edges only).
RIBBONS = [
    [(627, 220), (920, 400), (920, 554), (626, 373), (392, 524), (553, 666), (333, 553), (333, 400)],
    [(333, 639), (624, 816), (853, 668), (739, 565), (920, 652), (920, 795), (628, 977), (333, 795)],
]
# Four-point star: tips and the control points of its concave quadratic sides.
STAR_C = (627, 601)
STAR_TIPS = [(627, 527), (698, 601), (627, 686), (556, 601)]
STAR_CTRL = [(627, 601), (627, 601), (627, 601), (627, 601)]


def star_polygon(steps=24):
    pts = []
    for i, tip in enumerate(STAR_TIPS):
        nxt = STAR_TIPS[(i + 1) % 4]
        ctrl = (STAR_C[0] + (tip[0] - STAR_C[0] + nxt[0] - STAR_C[0]) * 0.0,
                STAR_C[1] + (tip[1] - STAR_C[1] + nxt[1] - STAR_C[1]) * 0.0)
        for s in range(steps):
            t = s / steps
            x = (1 - t) ** 2 * tip[0] + 2 * (1 - t) * t * ctrl[0] + t * t * nxt[0]
            y = (1 - t) ** 2 * tip[1] + 2 * (1 - t) * t * ctrl[1] + t * t * nxt[1]
            pts.append((x, y))
    return pts


def mask(size=SIZE):
    """Anti-aliased coverage of the white mark, 'L' image of `size` x `size`."""
    from PIL import ImageDraw
    big = Image.new("L", (SIZE * SS, SIZE * SS), 0)
    d = ImageDraw.Draw(big)
    for poly in RIBBONS + [star_polygon()]:
        d.polygon([(x * SS, y * SS) for x, y in poly], fill=255)
    return big.resize((size, size), Image.LANCZOS)


def orange_square(size):
    m = mask(size)
    img = Image.new("RGB", (size, size), ORANGE)
    img.paste((255, 255, 255), mask=m)
    return img


def white_transparent(size, crop=True):
    """White mark on a transparent background, cropped to the mark plus a small margin."""
    m = mask(SIZE)
    if crop:
        box = (300, 190, 954, 1010)  # mark bounds with padding
        side = max(box[2] - box[0], box[3] - box[1])
        cx, cy = (box[0] + box[2]) / 2, (box[1] + box[3]) / 2
        box = (int(cx - side / 2), int(cy - side / 2), int(cx + side / 2), int(cy + side / 2))
        m = m.crop(box)
    m = m.resize((size, size), Image.LANCZOS)
    img = Image.new("RGBA", (size, size), (255, 255, 255, 0))
    img.putalpha(m)
    return img


SVG_MARK = ("M" + " L".join(f"{x} {y}" for x, y in RIBBONS[0]) + "Z "
            "M" + " L".join(f"{x} {y}" for x, y in RIBBONS[1]) + "Z ")


def star_path():
    d = f"M{STAR_TIPS[0][0]} {STAR_TIPS[0][1]}"
    for i, tip in enumerate(STAR_TIPS):
        nxt = STAR_TIPS[(i + 1) % 4]
        cx = STAR_C[0] + (tip[0] - STAR_C[0] + nxt[0] - STAR_C[0]) * 0.0
        cy = STAR_C[1] + (tip[1] - STAR_C[1] + nxt[1] - STAR_C[1]) * 0.0
        d += f" Q{cx:.1f} {cy:.1f} {nxt[0]} {nxt[1]}"
    return d + "Z"


def write_svgs(out):
    path = SVG_MARK + star_path()
    (out / "subcraft-logo-mark.svg").write_text(
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{SIZE}" height="{SIZE}" viewBox="0 0 {SIZE} {SIZE}">'
        f'<path fill="#ffffff" d="{path}"/></svg>\n', encoding="utf-8")
    (out / "subcraft-logo-orange.svg").write_text(
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{SIZE}" height="{SIZE}" viewBox="0 0 {SIZE} {SIZE}">'
        f'<rect width="{SIZE}" height="{SIZE}" fill="#ff7800"/><path fill="#ffffff" d="{path}"/></svg>\n',
        encoding="utf-8")


def main():
    root = Path(__file__).resolve().parent.parent
    out = root / "branding"
    orange_square(SIZE).save(out / "subcraft-logo-orange-1254.png", optimize=True)
    white_transparent(1024).save(out / "subcraft-logo-mark-white-transparent.png", optimize=True)
    write_svgs(out)

    # Window icon (egui) and the in-app header mark.
    orange_square(128).convert("RGBA").save(root / "crates/ui/assets/icon.png", optimize=True)
    orange_square(96).convert("RGBA").save(root / "crates/ui/assets/logo-mark.png", optimize=True)
    # Linux launcher icon; macOS builds SubCraft.icns from it (scripts/build-macos.sh).
    orange_square(256).convert("RGBA").save(root / "packaging/linux/subcraft.png", optimize=True)
    # Windows exe / installer icon.
    sizes = [16, 24, 32, 48, 64, 128, 256]
    orange_square(256).convert("RGBA").save(
        root / "apps/subcraft/assets/icon.ico", format="ICO", sizes=[(s, s) for s in sizes])
    # GitHub social preview (1280 x 640): the mark beside the product name.
    social(out)


def social(out):
    from PIL import ImageDraw, ImageFont
    w, h = 1280, 640
    img = Image.new("RGB", (w, h), (30, 30, 30))
    sq = orange_square(400)
    img.paste(sq, (120, 120))
    d = ImageDraw.Draw(img)
    font = None
    for name in ("segoeuib.ttf", "arialbd.ttf", "DejaVuSans-Bold.ttf"):
        try:
            font = ImageFont.truetype(name, 104)
            break
        except OSError:
            continue
    font = font or ImageFont.load_default()
    d.text((580, 215), "SubCraft", fill=(244, 244, 245), font=font)
    d.rectangle((584, 350, 664, 358), fill=ORANGE)
    try:
        small = ImageFont.truetype("segoeui.ttf", 38)
    except OSError:
        small = ImageFont.load_default()
    d.text((584, 390), "3D texture painting", fill=(190, 190, 190), font=small)
    img.save(out / "subcraft-github-social-preview.png", optimize=True)


if __name__ == "__main__":
    main()
