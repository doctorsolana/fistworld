#!/usr/bin/env python3
"""Small text-free ledger frames and pennants; requires Pillow.

These are original vector-like shapes, not crops of reference UI. Draw at 4x,
then write optimized indexed PNGs. Nine-slice insets must retain a real centre.
"""
from pathlib import Path
from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parents[2] / 'client/assets/ui/ledger'
OUT.mkdir(parents=True, exist_ok=True)

def save(name, im, size):
    im = im.resize(size, Image.Resampling.LANCZOS)
    im.quantize(colors=96, method=Image.Quantize.FASTOCTREE).save(OUT/name, optimize=True)

def button(name, face):
    im = Image.new('RGBA', (384,192))
    d = ImageDraw.Draw(im)
    points=[(5,12),(20,3),(361,6),(380,16),(378,173),(362,186),(17,188),(4,176)]
    d.polygon(points,fill=(60,39,20,255))
    d.polygon([(12,15),(25,10),(357,12),(372,20),(370,169),(359,179),(23,180),(12,173)],fill=(148,115,65,255))
    d.polygon([(18,22),(30,18),(353,20),(365,25),(362,164),(355,171),(29,171),(20,165)],fill=face)
    d.line([(21,25),(353,23),(362,27)],fill=(242,220,165,125),width=3)
    d.line([(25,165),(356,165),(361,161)],fill=(62,36,16,110),width=4)
    save(name,im,(96,48))

button('button-paper.png',(224,205,167,255))
button('button-wood.png',(69,46,29,255))

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
