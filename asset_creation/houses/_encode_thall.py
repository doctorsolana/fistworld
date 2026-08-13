"""Compose the town-hall variant frames into one comparison sheet. Needs Pillow."""
import os, sys
from PIL import Image, ImageDraw, ImageFont
frames, sheets = sys.argv[1], sys.argv[2]
rows = [l.rstrip("\n").split("\t") for l in open(os.path.join(frames, "index.txt"))]

def font(size, bold=False):
    names = ("/System/Library/Fonts/Supplemental/Arial Bold.ttf",) if bold else ()
    for p in names + ("/System/Library/Fonts/Helvetica.ttc",
                      "/System/Library/Fonts/Supplemental/Arial.ttf"):
        if os.path.exists(p):
            try: return ImageFont.truetype(p, size)
            except Exception: pass
    return ImageFont.load_default()

tiles = [(r, Image.open(os.path.join(frames, f"{r[0]}.png")).convert("RGB")) for r in rows]
TW, TH = tiles[0][1].size
TITLE, CAP, GAP = 58, 76, 5
sheet = Image.new("RGB", (len(tiles) * (TW + GAP), TITLE + TH + CAP), (22, 22, 25))
d = ImageDraw.Draw(sheet)
d.text((14, 12), "Town hall (L3) — three massings   same camera, sun and 1.70 m villager   "
                 "grey = the forecourt each one implies",
       fill=(242, 242, 248), font=font(24, True))
for i, ((key, label, tris, size, area), im) in enumerate(tiles):
    x = i * (TW + GAP)
    sheet.paste(im, (x, TITLE))
    d.text((x + 12, TITLE + TH + 8), label, fill=(246, 246, 252), font=font(24, True))
    d.text((x + 12, TITLE + TH + 38), f"{int(tris):,} tri    {size} m",
           fill=(156, 156, 170), font=font(18))
    d.text((x + 12, TITLE + TH + 56), f"forecourt ~{int(float(area))} m2",
           fill=(150, 190, 160), font=font(18))
out = os.path.join(sheets, "town_hall_variants.png")
sheet.save(out)
print(f"  {os.path.basename(out):26s} {len(tiles)} variants  {os.path.getsize(out)/1024:.0f} KB")
