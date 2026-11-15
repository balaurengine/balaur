#!/usr/bin/env python3
"""Name the glyphs of a Phosphor face with Phosphor's own names.

Phosphor's fonts carry no glyph names (a `post` table of format 3), so a
widget's `icon = "plus"` has nothing to find. Phosphor names each glyph in
its web package's stylesheet, `.ph-fill.ph-plus:before { content: "\\e3d4" }`;
this writes those names into the face as Phosphor spells them
(`arrows-out-cardinal`). A codepoint the sheet names
twice keeps its first name, and a plain glyph an icon's name falls on is named
for its codepoint. Rerun after a Phosphor upgrade:

    curl -sSfLO https://cdn.jsdelivr.net/npm/@phosphor-icons/web@2.1.1/src/fill/style.css
    python3 scripts/phosphor_names.py style.css editor/fonts/Phosphor-Fill.ttf
"""

import re
import sys

from fontTools.ttLib import TTFont


def names_of(sheet: str) -> dict[int, str]:
    found: dict[int, str] = {}
    for name, code in re.findall(r'\.ph-[a-z]+\.ph-([a-z0-9-]+):before\s*\{\s*content:\s*"\\([0-9a-f]+)"', sheet):
        found.setdefault(int(code, 16), name)
    return found


def main(sheet_path: str, face_path: str) -> None:
    names = names_of(open(sheet_path, encoding="utf-8").read())
    # Read from one copy, renamed in another: a table read before the rename
    # keeps the old names and no longer finds its glyphs.
    probe = TTFont(face_path)
    order = probe.getGlyphOrder()
    cmap = probe.getBestCmap()
    by_glyph = {glyph: names[code] for code, glyph in cmap.items() if code in names}
    # A plain glyph an icon's name falls on (the letter `x` beside the icon
    # `x`) is named for its codepoint instead, as the AGL names one.
    taken = set(by_glyph.values())
    for code, glyph in cmap.items():
        if glyph not in by_glyph and glyph in taken:
            by_glyph[glyph] = f"uni{code:04X}"
    renamed = [by_glyph.get(glyph, glyph) for glyph in order]
    if len(set(renamed)) != len(renamed):
        sys.exit("two glyphs would share a name")
    face = TTFont(face_path)
    face["post"]  # read first: a named `post` sets the order it holds
    face.setGlyphOrder(renamed)
    for tag in face.keys():
        face[tag]
    face["post"].formatType = 2.0
    face["post"].extraNames = []
    face["post"].mapping = {}
    face.save(face_path)
    print(f"{face_path}: {sum(1 for a, b in zip(order, renamed) if a != b)} glyphs named")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
