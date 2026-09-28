#!/usr/bin/env python3
"""The macOS app icon: the light-ink mark on the dark plate the dock draws.

The geometry is `prepare` in crates/balaur_render/src/app_icon.rs and the
plate is chrome.rn's DARK_PLATE, so the Finder shows what the running dock
does. Writes editor/assets/balaur-app-icon.png:

  python3 scripts/app_icon.py
"""
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "editor" / "assets"
CANVAS, PLATE, RADIUS, LOGO = 1024, 824, 185, 660
DARK_PLATE = (20, 24, 29, 255)

canvas = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
low = (CANVAS - PLATE) // 2
ImageDraw.Draw(canvas).rounded_rectangle(
    (low, low, low + PLATE, low + PLATE), radius=RADIUS, fill=DARK_PLATE
)
mark = Image.open(ASSETS / "balaur-logo-dark.png").convert("RGBA")
mark = mark.resize((LOGO, LOGO), Image.Resampling.BICUBIC)
at = (CANVAS - LOGO) // 2
canvas.alpha_composite(mark, (at, at))
canvas.save(ASSETS / "balaur-app-icon.png")
