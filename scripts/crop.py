#!/usr/bin/env python3
"""Cut a rect out of a PNG. `scripts/views.sh` uses it to lift one editor
view out of a shot of the whole shell."""
import sys
from PIL import Image

src, dst, x, y, w, h = sys.argv[1:7]
im = Image.open(src)
x, y, w, h = (round(float(v)) for v in (x, y, w, h))
im.crop((x, y, min(x + w, im.width), min(y + h, im.height))).save(dst)
