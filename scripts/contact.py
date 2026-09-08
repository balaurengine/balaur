#!/usr/bin/env python3
"""A contact sheet of editor views, laid out the way they are seen: side
panels along a row, bottom panels down a column.

`scripts/views.sh` writes one per group, so a design pass can compare the
views against each other rather than one shot at a time."""
import sys
from PIL import Image

GAP = 16
INK = (24, 28, 35)

mode, dst, *paths = sys.argv[1:]
images = [Image.open(p) for p in paths if p]
if not images:
    sys.exit("no views to lay out")
if mode == "row":
    width = sum(i.width for i in images) + GAP * (len(images) + 1)
    height = max(i.height for i in images) + GAP * 2
else:
    width = max(i.width for i in images) + GAP * 2
    height = sum(i.height for i in images) + GAP * (len(images) + 1)
sheet = Image.new("RGB", (width, height), INK)
at = GAP
for im in images:
    sheet.paste(im, (at, GAP) if mode == "row" else (GAP, at))
    at += (im.width if mode == "row" else im.height) + GAP
sheet.save(dst)
