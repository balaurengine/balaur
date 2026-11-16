#!/usr/bin/env python3
"""Name the glyphs of a Phosphor face with Phosphor's own names.

Phosphor's fonts carry no glyph names (a `post` table of format 3), so a
widget's `icon = "plus"` has nothing to find. Phosphor names each glyph in
its web package's stylesheet, `.ph-fill.ph-plus:before { content: "\\e3d4" }`;
this writes those names into the face as Phosphor spells them
(`arrows-out-cardinal`). A codepoint the sheet names
twice keeps its first name, and a plain glyph an icon's name falls on is named
for its codepoint.

Given a second weight's sheet and face, it also copies that weight's icons in
under the names Phosphor's own files give them (`x-bold`), past the Basic
Plane, where no weight's codepoints fall. The editor's face is the Fill
weight with the Bold one beside it. Rerun after a Phosphor upgrade:

    base=https://cdn.jsdelivr.net/npm/@phosphor-icons/web@2.1.1/src
    curl -sSfL -o fill.css $base/fill/style.css
    curl -sSfL -o editor/fonts/Phosphor.ttf $base/fill/Phosphor-Fill.ttf
    curl -sSfL -o bold.css $base/bold/style.css
    curl -sSfL -o Phosphor-Bold.ttf $base/bold/Phosphor-Bold.ttf
    python3 scripts/phosphor_names.py fill.css editor/fonts/Phosphor.ttf bold.css Phosphor-Bold.ttf
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


# Where a second weight's icons go: the first Supplementary Private Use
# plane, at the same offset each has in the Basic Plane's private block.
PLANE = 0xF0000
BASIC_PRIVATE = 0xE000


def add_weight(face_path: str, sheet_path: str, other_path: str, weight: str) -> None:
    names = names_of(open(sheet_path, encoding="utf-8").read())
    face = TTFont(face_path)
    other = TTFont(other_path)
    if face["head"].unitsPerEm != other["head"].unitsPerEm:
        sys.exit("the two weights are drawn on different grids")
    order = face.getGlyphOrder()
    taken = set(order)
    cmap = other.getBestCmap()
    added: list[tuple[int, str, str]] = []
    for code, glyph in sorted(cmap.items()):
        if code not in names or code < BASIC_PRIVATE:
            continue
        name = f"{names[code]}-{weight}"
        if name in taken:
            sys.exit(f"`{name}` is already a glyph of {face_path}")
        taken.add(name)
        added.append((PLANE + code - BASIC_PRIVATE, name, glyph))
    glyf, hmtx = face["glyf"], face["hmtx"]
    grown = order + [name for _, name, _ in added]
    face.setGlyphOrder(grown)
    glyf.glyphOrder = grown
    for _, name, glyph in added:
        drawn = other["glyf"][glyph]
        drawn.expand(other["glyf"])
        glyf.glyphs[name] = drawn
        hmtx[name] = other["hmtx"][glyph]
    # One subtable that reaches past the Basic Plane, holding every mapping.
    mapped = dict(face.getBestCmap())
    mapped.update({code: name for code, name, _ in added})
    from fontTools.ttLib.tables._c_m_a_p import CmapSubtable
    wide = CmapSubtable.newSubtable(12)
    wide.platformID, wide.platEncID, wide.language = 3, 10, 0
    wide.cmap = mapped
    narrow = CmapSubtable.newSubtable(4)
    narrow.platformID, narrow.platEncID, narrow.language = 3, 1, 0
    narrow.cmap = {code: name for code, name in mapped.items() if code <= 0xFFFF}
    face["cmap"].tables = [narrow, wide]
    face["maxp"].numGlyphs = len(face.getGlyphOrder())
    face["post"].formatType = 2.0
    face["post"].extraNames = []
    face["post"].mapping = {}
    face.save(face_path)
    print(f"{face_path}: {len(added)} {weight} glyphs added")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
    if len(sys.argv) == 5:
        add_weight(sys.argv[2], sys.argv[3], sys.argv[4], "bold")
