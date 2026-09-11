#!/usr/bin/env python3
"""Small text-free ledger frames and pennants; requires Pillow.

These are original vector-like shapes, not crops of reference UI. Draw at 4x,
then write optimized indexed PNGs. Nine-slice insets must retain a real centre.
"""
from pathlib import Path
import math
import random
from PIL import Image, ImageDraw, ImageFilter

OUT = Path(__file__).resolve().parents[2] / 'client/assets/ui/ledger'
OUT.mkdir(parents=True, exist_ok=True)

def save(name, im, size):
    im = im.resize(size, Image.Resampling.LANCZOS)
    im.quantize(colors=96, method=Image.Quantize.FASTOCTREE).save(OUT/name, optimize=True)

def button(face, brass, seed, width=192):
    """Authored leather/paper inset and worn brass, with no painted lettering.

    Coordinates are runtime pixels at 192x64. Square controls remap only the
    centre span, preserving the authored corner widths instead of crushing the
    brass into subpixel lines. The shallow perimeter kinks,
    broken highlights and exposed edges stay legible after nine-slicing. Wear
    stays around the rim; the quiet centre is reserved for native text.
    """
    scale = 4
    size = (width * scale, 64 * scale)
    im = Image.new('RGBA', size)
    rng = random.Random(seed)

    def points(vertices):
        def remap(x):
            if width == 192 or x <= 8:
                return x
            if x >= 184:
                return x + width - 192
            return 8 + (x - 8) * (width - 16) / 176
        return [(round(remap(x) * scale), round(y * scale)) for x, y in vertices]

    def stroke(vertices, colour, width=0.65):
        ImageDraw.Draw(im).line(points(vertices), fill=colour,
                               width=max(1, round(width * scale)), joint='curve')

    def material(vertices, colour, grain=4, relief=0):
        mask = Image.new('L', size)
        ImageDraw.Draw(mask).polygon(points(vertices), fill=255)
        # Seeded broad mottling and fine grain keep regeneration byte-stable.
        cloud_width = max(8, width // 4)
        cloud = Image.new('L', (cloud_width, 16))
        cloud.putdata([rng.randrange(70, 187) for _ in range(cloud_width * 16)])
        cloud = cloud.resize(size, Image.Resampling.BICUBIC)
        pixels = []
        for i, value in enumerate(cloud.tobytes()):
            y = (i // size[0]) / size[1]
            light = grain * ((value - 128) / 45 + rng.uniform(-0.3, 0.3))
            light += relief * (math.sin(y * math.pi) - 0.45)
            pixels.append(tuple(max(0, min(255, round(c + light))) for c in colour))
        layer = Image.new('RGB', size)
        layer.putdata(pixels)
        im.paste(layer, (0, 0), mask)

    rim = [(2, 6), (5, 2), (22, 1.5), (48, 2), (51, 3), (55, 2.2),
           (103, 1.5), (141, 2.2), (146, 1.6), (185, 2.1), (190, 5),
           (190.5, 20), (189.5, 25), (190, 54), (188, 60), (167, 60.5),
           (163, 59.5), (159, 60.8), (110, 61), (66, 60.1), (29, 61),
           (6, 60.5), (2, 57), (1.5, 35), (2.3, 29)]
    shadow = Image.new('L', size)
    ImageDraw.Draw(shadow).polygon(points([(x, y + 1.5) for x, y in rim]), fill=165)
    shadow = shadow.filter(ImageFilter.GaussianBlur(0.65 * scale))
    im.paste((18, 12, 7, 255), (0, 0), shadow)
    material(rim, (49, 32, 19), grain=3)
    material([(3.5, 6), (6, 3.2), (48, 3.5), (52, 4.2), (57, 3.6),
              (103, 3), (145, 3.2), (185, 3.8), (188.5, 6), (188, 55),
              (186, 58.4), (166, 58.8), (162, 58.2), (158, 59),
              (66, 58.5), (7, 59), (4, 56.5), (3.6, 34)], brass, grain=13, relief=6)
    # Recessed dark seam separates the face from the hand-beaten brass rim.
    material([(6, 8), (9, 5.8), (103, 5.8), (184, 6.5), (186, 8),
              (185.5, 54.5), (183, 56.5), (8, 56.8), (6, 54)],
             (38, 25, 15), grain=3)
    material([(7.5, 9), (10, 7), (103, 7), (183, 7.8), (184.5, 9),
              (184, 53.7), (182.5, 55), (9, 55.2), (7.5, 53)],
             face, grain=5, relief=13)

    # Interrupted bevels: long flawless rules were what made the old face flat.
    highlight = tuple(min(255, c + 54) for c in brass) + (255,)
    worn = tuple(min(255, c + 28) for c in brass) + (255,)
    stroke([(5, 8), (6.5, 4.4), (22, 3.7), (36, 4)], highlight, 0.9)
    stroke([(39, 4), (48, 4.1)], worn)
    stroke([(59, 4.3), (101, 3.8), (129, 4.2)], highlight)
    stroke([(138, 4.3), (166, 4.2), (182, 4.8)], worn)
    stroke([(188, 8), (187.5, 19)], worn)
    stroke([(4.8, 14), (4.7, 28)], worn)
    stroke([(8, 58), (29, 58.4), (48, 58)], (63, 39, 20, 255), 1.0)
    stroke([(58, 58), (111, 58.6), (151, 58.3)], (66, 42, 22, 255), 1.0)
    stroke([(170, 58), (185, 57.4), (187, 54)], worn, 0.8)
    stroke([(10, 8.5), (29, 8.5)], tuple(min(255, c + 22) for c in face) + (255,))
    stroke([(23, 54), (69, 54), (115, 54.3)], tuple(max(0, c - 18) for c in face) + (255,))

    # Sparse oxidation and chips; these are authored marks, not damaged text.
    for vertices in [[(17, 3.1), (19, 4.5), (22, 3.2)],
                     [(77, 3.1), (80, 4.5), (82, 3.4)],
                     [(155, 3.2), (158, 4.7), (160, 3.4)],
                     [(92, 59.5), (96, 57.8), (98, 59.7)],
                     [(188.5, 42), (186.8, 44), (188.5, 47)]]:
        stroke(vertices, (75, 65, 41, 255), 0.8)
    stroke([(11, 4.5), (13, 6.5)], highlight, 0.6)
    stroke([(178, 56), (181, 58)], highlight, 0.75)
    return im


save('button-paper.png', button((224, 205, 167), (146, 111, 63), 71), (192, 64))
# One shared atlas keeps the amber leather from over-brightening its brass rim.
# skin.rs selects an exact 192x64 rect; no extra handles/materials per button.
atlas = Image.new('RGBA', (768, 512))
atlas.paste(button((46, 34, 26), (139, 106, 60), 83), (0, 0))
atlas.paste(button((135, 76, 27), (181, 129, 64), 83), (0, 256))
save('button-wood.png', atlas, (192, 128))
save('button-close.png', button((46, 34, 26), (139, 106, 60), 83, width=64), (64, 64))

im=Image.new('RGBA',(512,512));d=ImageDraw.Draw(im)
for inset,fill in [(4,(39,28,16,230)),(12,(113,81,41,255)),(20,(201,167,99,255)),(27,(107,79,43,255)),(33,(227,204,150,255)),(38,(59,42,25,255))]:
    d.ellipse((inset,inset,511-inset,511-inset),fill=fill)
d.ellipse((45,45,466,466),fill=(0,0,0,0))
save('portrait-frame.png',im,(128,128))

im=Image.new('RGBA',(256,320));d=ImageDraw.Draw(im)
d.polygon([(16,9),(240,9),(234,284),(129,309),(22,280)],fill=(91,59,27,255))
d.polygon([(25,17),(231,18),(224,276),(129,299),(32,273)],fill=(179,142,76,255))
d.polygon([(32,24),(224,25),(216,269),(129,290),(40,266)],fill=(49,67,64,255))
for x in [48,82,116,150,184,212]: d.line([(x,30),(x-4,266)],fill=(76,88,76,100),width=2)
save('pennant.png',im,(64,80))


# A shared, text-free navigation glyph pair. Gold alpha masks tint naturally.
for name in ['army', 'retinue']:
    im=Image.new('RGBA',(192,192));d=ImageDraw.Draw(im)
    gold=(215,182,120,255)
    if name=='retinue':
        for x,y,r in [(45,58,19),(143,58,19),(94,48,24)]:
            d.ellipse((x-r,y-r,x+r,y+r),fill=gold)
        for box in [(15,85,74,151),(116,85,177,151),(56,80,134,169)]:
            d.rounded_rectangle(box,22,fill=gold)
    else:
        # Complete hilts, guards and tapered blades, no dangling segments.
        d.polygon([(23,21),(58,35),(139,118),(121,136),(38,56)],fill=gold)
        d.line([(110,143),(145,108)],fill=gold,width=12)
        d.line([(129,128),(164,163)],fill=gold,width=13)
        d.ellipse((155,155,176,176),fill=gold)
        d.polygon([(169,21),(134,35),(53,118),(71,136),(154,56)],fill=gold)
        d.line([(82,143),(47,108)],fill=gold,width=12)
        d.line([(63,128),(28,163)],fill=gold,width=13)
        d.ellipse((16,155,37,176),fill=gold)
    save(name+'.png',im,(48,48))
