"""Generate original minimal test fonts; requires fonttools, never run by tests.

These geometric fixtures are licensed under the repository's GPL-3.0-or-later.
No outlines or font data are copied from third-party fonts.
"""
from pathlib import Path
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen


def build(name, cmap, box, advance):
    builder = FontBuilder(1000, isTTF=True)
    builder.setupGlyphOrder([".notdef", "mark", "space"])
    builder.setupCharacterMap(cmap)
    glyphs = {}
    for glyph in [".notdef", "mark", "space"]:
        pen = TTGlyphPen(None)
        if glyph != "space":
            x0, y0, x1, y1 = box
            pen.moveTo((x0, y0))
            pen.lineTo((x1, y0))
            pen.lineTo((x1, y1))
            pen.lineTo((x0, y1))
            pen.closePath()
        glyphs[glyph] = pen.glyph()
    builder.setupGlyf(glyphs)
    builder.setupHorizontalMetrics({g: (advance, 0) for g in glyphs})
    builder.setupHorizontalHeader(ascent=800, descent=-200)
    builder.setupNameTable({"familyName": name, "styleName": "Regular"})
    builder.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200)
    builder.setupPost()
    builder.setupMaxp()
    builder.font.recalcTimestamp = False
    builder.font["head"].created = builder.font["head"].modified = 2082844800
    builder.save(Path(__file__).with_name(name + ".ttf"))


build("primary", {ord("M"): "mark", ord("A"): "mark", 32: "space"}, (0, 0, 500, 700), 600)
# Deliberately over-wide and taller than the text cell to exercise fitting.
build("icons", {0xF269: "mark", ord("A"): "mark"}, (-200, -400, 1400, 1200), 1600)
