"""Compose the hall comparison into one sheet. Needs Pillow."""
import os
import sys

from PIL import Image, ImageDraw, ImageFont

tmp, out = sys.argv[1], sys.argv[2]
rows = [ln.rstrip("\n").split("\t") for ln in open(os.path.join(tmp, "meta.txt")) if ln.strip()]
VIEWS = [("rts", "game camera angle"), ("eye", "from the street")]


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


first = Image.open(os.path.join(tmp, f"{rows[0][0]}_rts.png"))
TW, TH = first.size
GAP, TITLE, CAP = 6, 54, 62
W = len(rows) * TW + (len(rows) - 1) * GAP
H = TITLE + len(VIEWS) * (TH + CAP + GAP)
sheet = Image.new("RGB", (W, H), (20, 20, 23))
d = ImageDraw.Draw(sheet)
d.text((16, 14), "moot hall -> village hall   same camera, same sun, same distance",
       fill=(243, 243, 249), font=font(26, True))

for vi, (vkey, vlabel) in enumerate(VIEWS):
    y = TITLE + vi * (TH + CAP + GAP)
    for ci, (mesh, label, tris, size) in enumerate(rows):
        x = ci * (TW + GAP)
        p = os.path.join(tmp, f"{mesh}_{vkey}.png")
        if os.path.exists(p):
            sheet.paste(Image.open(p).convert("RGB"), (x, y))
        d.text((x + 14, y + 12), vlabel, fill=(214, 214, 226), font=font(20))
        d.text((x + 14, y + TH + 8), label, fill=(248, 248, 253), font=font(23, True))
        d.text((x + 14, y + TH + 36), f"{tris} tris    {size}", fill=(152, 152, 166), font=font(18))

path = os.path.join(out, "hall_upgrade.png")
sheet.save(path)
print(f"  {os.path.basename(path):22s} {os.path.getsize(path)/1024:.0f} KB")
