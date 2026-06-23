"""Render the Duckle app icon: the exact "d" brand art on a warm rounded tile.

Does NOT redraw the mark - it composites the user's actual artwork
(apps/desktop/icons/icon-mark.png, a transparent/trimmed copy of the brand "d")
centred on the cream tile, so the icon is pixel-faithful to the source. Run from
the repo root:

    python scripts/render_icon.py
    cargo tauri icon apps/desktop/icons/icon-source.png   # from apps/desktop
"""

from PIL import Image

S = 1024                              # output size
CREAM = (0xFB, 0xF3, 0xE8, 255)       # warm tile background
MARK = "apps/desktop/icons/icon-mark.png"
OUT = "apps/desktop/icons/icon-source.png"


def main():
    tile = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    # Warm rounded tile (transparent corners -> rounded app icon).
    from PIL import ImageDraw
    ImageDraw.Draw(tile).rounded_rectangle([0, 0, S - 1, S - 1], radius=int(0.20 * S), fill=CREAM)

    mark = Image.open(MARK).convert("RGBA")
    # Fit the mark to ~62% of the tile, preserving its aspect ratio.
    target = int(0.62 * S)
    scale = target / max(mark.width, mark.height)
    mark = mark.resize((round(mark.width * scale), round(mark.height * scale)), Image.LANCZOS)
    ox = (S - mark.width) // 2
    oy = (S - mark.height) // 2
    tile.alpha_composite(mark, (ox, oy))

    tile.save(OUT)
    print("wrote", OUT, tile.size, "(mark", mark.size, ")")


if __name__ == "__main__":
    main()
