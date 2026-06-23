"""Render the Duckle "d" logo to a high-res PNG for `tauri icon`.

Reproduces the brand mark (docs/assets/duckle-logo-light.svg) exactly: a warm
rounded tile with the two-tone lowercase "d" - a large peach bowl ring with a
concentric counter, an orange ascender stem, and a soft translucent overlap where
the stem crosses the bowl. Geometry is the same 64-unit viewBox the SVG uses.
Drawn at 4x and downscaled with LANCZOS for smooth edges. Run from the repo root:

    python scripts/render_icon.py
    cargo tauri icon apps/desktop/icons/icon-source.png   # from apps/desktop
"""

from PIL import Image, ImageDraw

S = 1024            # output size
SS = 4              # supersample factor
W = S * SS          # working size

# Brand palette (sampled from the logo art).
CREAM = (0xFB, 0xF3, 0xE8, 255)   # warm tile background
PEACH = (0xF6, 0xBA, 0x78, 255)   # bowl ring
ORANGE = (0xEA, 0x7E, 0x42, 255)  # ascender stem
OVERLAP = (0xD9, 0x74, 0x2F, 115)  # ~0.45 alpha deepening where they cross

# 64-unit mark viewBox -> canvas. The glyph (bbox x11..53, y4..60) is centred on
# (32, 32); scale it to ~60% of the tile height.
SCALE = 0.60 * W / 56.0


def mx(x):
    return W / 2 + (x - 32) * SCALE


def my(y):
    return W / 2 + (y - 32) * SCALE


def msz(v):
    return v * SCALE


def main():
    img = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # Warm rounded tile (transparent corners -> rounded app icon).
    d.rounded_rectangle([0, 0, W - 1, W - 1], radius=int(0.20 * W), fill=CREAM)

    # Bowl outer disc (peach), then punch the concentric counter to tile colour.
    cx, cy, r = mx(31.5), my(39.5), msz(20.5)
    d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=PEACH)
    cr = msz(11.1)
    d.ellipse([cx - cr, cy - cr, cx + cr, cy + cr], fill=CREAM)

    # Ascender stem (orange rounded bar): x 42.6..53, y 4..60.
    stem = [mx(42.6), my(4), mx(53.0), my(60)]
    srad = msz(5.2)
    d.rounded_rectangle(stem, radius=srad, fill=ORANGE)

    # Soft overlap: the part of the stem inside the bowl disc, translucent.
    overlay = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    ImageDraw.Draw(overlay).rounded_rectangle(stem, radius=srad, fill=OVERLAP)
    circle = Image.new("L", (W, W), 0)
    ImageDraw.Draw(circle).ellipse([cx - r, cy - r, cx + r, cy + r], fill=255)
    kept = Image.composite(overlay.getchannel("A"), Image.new("L", (W, W), 0), circle)
    overlay.putalpha(kept)
    img = Image.alpha_composite(img, overlay)

    img = img.resize((S, S), Image.LANCZOS)
    out = "apps/desktop/icons/icon-source.png"
    img.save(out)
    print("wrote", out, img.size)


if __name__ == "__main__":
    main()
