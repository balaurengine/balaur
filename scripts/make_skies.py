#!/usr/bin/env python3
"""Write the library's skies. They are gradients rather than photographs so
the download stays kilobytes and nothing carries a third-party licence."""

import pathlib

from PIL import Image

OUT = pathlib.Path(__file__).resolve().parent.parent / "editor" / "library" / "skies"
W, H = 512, 256

# name -> (zenith, horizon, ground), each linear sRGB 0-255.
SKIES = {
    "clear-day": ((0x3D, 0x76, 0xC9), (0xC7, 0xDC, 0xF0), (0x6B, 0x6A, 0x62)),
    "dusk": ((0x1B, 0x1E, 0x3E), (0xE8, 0x8A, 0x4E), (0x2A, 0x22, 0x24)),
    "overcast": ((0x8D, 0x96, 0x9E), (0xC5, 0xC9, 0xCC), (0x5B, 0x5C, 0x5A)),
}


def mix(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def band(zenith, horizon, ground, y):
    """An equirectangular column: zenith at the top, horizon at the middle."""
    t = y / (H - 1)
    if t < 0.5:
        return mix(zenith, horizon, (t / 0.5) ** 0.6)
    return mix(horizon, ground, ((t - 0.5) / 0.5) ** 0.5)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    for name, colors in SKIES.items():
        image = Image.new("RGB", (W, H))
        pixels = image.load()
        for y in range(H):
            row = band(*colors, y)
            for x in range(W):
                pixels[x, y] = row
        path = OUT / f"{name}.png"
        image.save(path, optimize=True)
        print(f"{path.name}: {path.stat().st_size} bytes")


if __name__ == "__main__":
    main()
