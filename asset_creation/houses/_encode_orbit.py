"""Compose the orbit frames into one sheet. Needs Pillow."""
import os, sys
from PIL import Image, ImageDraw, ImageFont
frames, sheets = sys.argv[1], sys.argv[2]
lines = open(os.path.join(frames, "index.txt")).read().split("\n")
names = [l for l in lines if l and not l.startswith("#")]
asset = next((l[1:] for l in lines if l.startswith("#")), "asset")
def font(sz, bold=False):
    c = ("/System/Library/Fonts/Supplemental/Arial Bold.ttf",) if bold else ()
    for p in c + ("/System/Library/Fonts/Helvetica.ttc",):
        if os.path.exists(p):
            try: return ImageFont.truetype(p, sz)
            except Exception: pass
    return ImageFont.load_default()
ims = [(n, Image.open(os.path.join(frames, f"{n}.png")).convert("RGB")) for n in names]
TW, TH = ims[0][1].size
COLS, GAP, TITLE, CAP = 5, 4, 46, 26
rows = (len(ims) + COLS - 1) // COLS
sheet = Image.new("RGB", (COLS*(TW+GAP), TITLE + rows*(TH+CAP+GAP)), (22, 22, 25))
d = ImageDraw.Draw(sheet)
d.text((14, 12), f"{asset} — orbit   same lens, sun and distance at every angle",
       fill=(242, 242, 248), font=font(24, True))
for i, (n, im) in enumerate(ims):
    x, y = (i % COLS)*(TW+GAP), TITLE + (i//COLS)*(TH+CAP+GAP)
    sheet.paste(im, (x, y))
    d.text((x+10, y+TH+4), n, fill=(232, 232, 240), font=font(19, True))
out = os.path.join(sheets, f"{asset}_orbit.png")
sheet.save(out)
print(f"  {os.path.basename(out):26s} {len(ims)} views  {os.path.getsize(out)/1024:.0f} KB")
