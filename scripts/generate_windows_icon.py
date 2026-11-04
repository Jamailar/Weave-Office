#!/usr/bin/env python3
"""Render the Windows app icon (.ico) and the window icon (256 px PNG) from the approved master.

The master (`assets/app-icon/weave-office.png`) is a pale rounded tile on a white backdrop. Like
the macOS generator, this crops to the tile and makes everything outside its rounded corners
transparent, so the icon sits cleanly on light and dark taskbars and Start menus.

Requires Pillow. Run from anywhere: `python3 scripts/generate_windows_icon.py`.
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
MASTER = ROOT / "assets/app-icon/weave-office.png"
ICO = ROOT / "assets/app-icon/weave-office.ico"
PNG_256 = ROOT / "assets/app-icon/weave-office-256.png"
# Tile bounds in the 1254 px master: the macOS generator's 65..959 clip of a 1024 px canvas.
TILE = (80, 80, 1174, 1174)
RADIUS = 269
SIZES = (16, 20, 24, 32, 40, 48, 64, 128, 256)
SUPERSAMPLE = 4


def tile() -> Image.Image:
    master = Image.open(MASTER).convert("RGBA")
    if master.size != (1254, 1254):
        raise SystemExit(f"unexpected master size {master.size}; update TILE and RADIUS")
    left, top, right, bottom = TILE
    side = right - left
    mask = Image.new("L", (side * SUPERSAMPLE, side * SUPERSAMPLE), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        (0, 0, side * SUPERSAMPLE - 1, side * SUPERSAMPLE - 1), RADIUS * SUPERSAMPLE, fill=255)
    cropped = master.crop(TILE)
    cropped.putalpha(mask.resize((side, side), Image.LANCZOS))
    # A 2% transparent margin, as Windows icons usually have, so the tile's edge is not clipped.
    pad = round(side * 0.02)
    canvas = Image.new("RGBA", (side + 2 * pad, side + 2 * pad), (0, 0, 0, 0))
    canvas.paste(cropped, (pad, pad), cropped)
    return canvas


def main() -> None:
    image = tile()
    frames = [image.resize((size, size), Image.LANCZOS) for size in SIZES]
    frames[-1].save(PNG_256, optimize=True)
    frames[-1].save(ICO, format="ICO", sizes=[(size, size) for size in SIZES],
                    append_images=frames[:-1])
    print(ICO)
    print(PNG_256)


if __name__ == "__main__":
    main()
