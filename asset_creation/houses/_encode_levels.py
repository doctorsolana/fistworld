"""Compose the civic-ladder frames into one sheet, with an empty slot for the level that does not
exist yet. Needs Pillow."""
import os
import sys

from PIL import Image, ImageDraw, ImageFont

frames, sheets = sys.argv[1], sys.argv[2]
rows = []
with open(os.path.join(frames, "index.txt")) as fh:
    for line in fh:
        rows.append(line.rstrip("\n").split("\t"))


def font(size, bold=False):
    names = ("/System/Library/Fonts/Supplemental/Arial Bold.ttf",) if bold else ()
    for p in names + ("/System/Library/Fonts/Helvetica.ttc",
                      "/System/Library/Fonts/Supplemental/Arial.ttf"):
        if os.path.exists(p):
            try:
                return ImageFont.truetype(p, size)
            except Exception:
                pass
    return ImageFont.load_default()


tiles = [(r, Image.open(os.path.join(frames, f"{r[0]}.png")).convert("RGB")) for r in rows]
TW, TH = tiles[0][1].size
TITLE, CAP, GAP = 58, 74, 5
cols = len(tiles) + 1                     # +1 for the not-yet-built level
W = cols * (TW + GAP)
sheet = Image.new("RGB", (W, TITLE + TH + CAP), (22, 22, 25))
d = ImageDraw.Draw(sheet)
d.text((14, 12), "Civic ladder   same camera, sun and 1.70 m villager for every level",
       fill=(242, 242, 248), font=font(25, True))

for i, ((want, label, tier, tris, size), im) in enumerate(tiles):
    x = i * (TW + GAP)
    sheet.paste(im, (x, TITLE))
    d.text((x + 12, TITLE + TH + 8), label, fill=(246, 246, 252), font=font(23, True))
    d.text((x + 12, TITLE + TH + 36), f"{tier}", fill=(150, 200, 160), font=font(18))
    d.text((x + 12, TITLE + TH + 54), f"{int(tris):,} tri   {size} m",
           fill=(156, 156, 170), font=font(17))

# the empty plot
x = len(tiles) * (TW + GAP)
d.rectangle([x, TITLE, x + TW, TITLE + TH], fill=(31, 31, 36))
for gy in range(TITLE, TITLE + TH, 26):
    d.line([(x + 8, gy), (x + TW - 8, gy)], fill=(40, 40, 46))
d.text((x + TW // 2 - 96, TITLE + TH // 2 - 22), "not built yet",
       fill=(96, 96, 108), font=font(28, True))
d.text((x + 12, TITLE + TH + 8), "L3  Town Hall", fill=(120, 120, 132), font=font(23, True))
d.text((x + 12, TITLE + TH + 36), "town", fill=(92, 118, 100), font=font(18))
d.text((x + 12, TITLE + TH + 54), "the ladder's next rung", fill=(100, 100, 112), font=font(17))

out = os.path.join(sheets, "civic_levels.png")
sheet.save(out)
print(f"  {os.path.basename(out):22s} {len(tiles)} built + 1 empty  "
      f"{os.path.getsize(out)/1024:.0f} KB")
