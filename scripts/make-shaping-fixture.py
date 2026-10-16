"""Regenerate the original synthetic OpenType test font (fontTools 4.54.1)."""
from pathlib import Path
from fontTools.fontBuilder import FontBuilder
from fontTools.ttLib import newTable
from fontTools.ttLib.tables.S_V_G_ import SVGDocument
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.feaLib.builder import addOpenTypeFeaturesFromString

order = ['.notdef', 'space', 'M', 'f', 'i', 'fi', 'e', 'acute', 'wide', 'ghostspace']
font = FontBuilder(1000, isTTF=True)
font.setupGlyphOrder(order)
font.setupCharacterMap({32:'space', 77:'M', 102:'f', 105:'i', 101:'e', 0x301:'acute', 0x754c:'wide', 0x2007:'ghostspace', 0x627:'f'})
glyphs = {}
for name in order:
    pen = TTGlyphPen(None)
    if name not in ('space','ghostspace'):
        width = 1100 if name in ('fi','wide') else 450
        if name == 'acute':
            pen.moveTo((0,0)); pen.lineTo((130,0)); pen.lineTo((230,170)); pen.lineTo((100,170))
        else:
            pen.moveTo((50,0)); pen.lineTo((width,0)); pen.lineTo((width,650)); pen.lineTo((50,650))
        pen.closePath()
    glyphs[name] = pen.glyph()
font.setupGlyf(glyphs)
font.setupHorizontalMetrics({name:(0 if name=='acute' else 1200 if name in ('fi','wide') else 600, 0) for name in order})
font.setupHorizontalHeader(ascent=1000, descent=-400)
font.setupOS2(sTypoAscender=1000,sTypoDescender=-400,usWinAscent=1000,usWinDescent=400)
font.setupNameTable({'familyName':'Whostty Shaping Fixture','styleName':'Regular','uniqueFontIdentifier':'WhosttyShapingFixture','fullName':'Whostty Shaping Fixture','psName':'WhosttyShapingFixture'})
font.setupPost(isFixedPitch=1)
font.setupMaxp()
addOpenTypeFeaturesFromString(font.font, '''
feature liga { sub f i by fi; } liga;
markClass acute <anchor 0 0> @top;
feature mark { pos base e <anchor 300 750> mark @top; } mark;
''')
font.font['SVG '] = newTable('SVG ')
font.font['SVG '].docList = [SVGDocument('<svg xmlns="http://www.w3.org/2000/svg"><path fill="red" d="M0 0h100v100z"/></svg>', 0, 0)]
font.font['head'].created = font.font['head'].modified = 3800000000
font.font.recalcTimestamp = False
font.save(Path(__file__).resolve().parents[1] / 'crates/desktop/tests/fixtures/shaping.ttf')
