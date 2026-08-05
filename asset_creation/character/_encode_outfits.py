"""Pack wardrobe renders into labelled sheets. Blender has no Pillow; system python3 does."""
import json, os, sys
from PIL import Image, ImageDraw, ImageFont

frames, sheets = sys.argv[1], sys.argv[2]
# Written by preview_wardrobe_v2.py describing what it actually rendered. Keeping a second copy of
# the item list and the outfit captions here is how sheets ended up labelled with the wrong outfits.
with open(os.path.join(frames, "index.json")) as fh:
    index = json.load(fh)

def font(size):
    for path in ("/System/Library/Fonts/Helvetica.ttc",
                 "/System/Library/Fonts/Supplemental/Arial.ttf"):
        if os.path.exists(path):
            try:
                return ImageFont.truetype(path, size)
            except Exception:
                pass
    return ImageFont.load_default()

def grid(tiles, out, cols, labels=None, pad=34):
    """tiles: list of PIL images. labels: one caption per PAIR of tiles (front/back)."""
    w, h = tiles[0].size
    rows = (len(tiles) + cols - 1) // cols
    band = pad if labels else 0
    sheet = Image.new("RGB", (w * cols, (h + band) * rows), (22, 22, 24))
    d = ImageDraw.Draw(sheet)
    f = font(22)
    for i, im in enumerate(tiles):
        r, c = i // cols, i % cols
        sheet.paste(im, (c * w, r * (h + band) + band))
    if labels:
        for r, text in enumerate(labels):
            d.text((10, r * (h + band) + 7), text, fill=(232, 232, 236), font=f)
    sheet.save(out)
    print(f"  {os.path.basename(out):16s} {len(tiles)} tiles  {os.path.getsize(out)/1024:.0f} KB")

# --- per-item catalogue: 2 items per row (front, back, front, back) ---
ITEMS = index["items"]
tiles, labels = [], []
for i in range(0, len(ITEMS), 2):
    pair = ITEMS[i:i + 2]
    for it in pair:
        for v in ("front", "back"):
            p = os.path.join(frames, f"item_{it}_{v}.png")
            if os.path.exists(p):
                tiles.append(Image.open(p).convert("RGB"))
    labels.append("   ".join(f"{it}  (front / back)" for it in pair))
if tiles:
    grid(tiles, os.path.join(sheets, "wardrobe.png"), 4, labels)

# --- outfit combinations ---
tags = index["outfits"]
tiles, labels = [], []
for t, cap in tags:
    got = False
    for v in ("front", "back"):
        p = os.path.join(frames, f"fit_{t}_{v}.png")
        if os.path.exists(p):
            tiles.append(Image.open(p).convert("RGB")); got = True
    if got:
        labels.append(f"{cap}  (front / back)")
if tiles:
    grid(tiles, os.path.join(sheets, "outfits.png"), 2, labels)
