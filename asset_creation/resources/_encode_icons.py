"""Contact sheet of the shipped icons, on a checker so transparency is visible. Needs Pillow."""
import os, sys
from PIL import Image, ImageDraw, ImageFont

icons, sheets = sys.argv[1], sys.argv[2]
ORDER = ["wood.png", "wheat.png", "fish.png", "stone.png", "iron.png",
         "axe.png", "hammer.png", "scythe.png"]


def font(size):
    for p in ("/System/Library/Fonts/Helvetica.ttc", "/System/Library/Fonts/Supplemental/Arial.ttf"):
        if os.path.exists(p):
            try:
                return ImageFont.truetype(p, size)
            except Exception:
                pass
    return ImageFont.load_default()


def checker(w, h, s=16):
    im = Image.new("RGB", (w, h), (58, 58, 62))
    d = ImageDraw.Draw(im)
    for y in range(0, h, s):
        for x in range(0, w, s):
            if (x // s + y // s) % 2:
                d.rectangle([x, y, x + s - 1, y + s - 1], fill=(72, 72, 77))
    return im


tiles = [(n, Image.open(os.path.join(icons, n)).convert("RGBA")) for n in ORDER
         if os.path.exists(os.path.join(icons, n))]
if not tiles:
    raise SystemExit("no icons found")

TH = 200
BAND, TITLE = 30, 40
w = TH * len(tiles)
sheet = Image.new("RGB", (w, TITLE + TH + BAND), (24, 24, 26))
d = ImageDraw.Draw(sheet)
d.text((12, 10), f"carried resource icons   {tiles[0][1].size[0]}x{tiles[0][1].size[1]} RGBA"
                 f"   transparent (checker shown behind)", fill=(240, 240, 245), font=font(20))
for i, (name, im) in enumerate(tiles):
    small = im.resize((TH, TH), Image.LANCZOS)
    bg = checker(TH, TH)
    bg.paste(small, (0, 0), small)
    sheet.paste(bg, (i * TH, TITLE))
    d.text((i * TH + 10, TITLE + TH + 6), name, fill=(225, 225, 232), font=font(18))
out = os.path.join(sheets, "resource_icons.png")
sheet.save(out)
print(f"  {os.path.basename(out):22s} {len(tiles)} icons  {os.path.getsize(out)/1024:.0f} KB")
