"""Pack work-clip frames into one labelled strip per clip. Blender has no Pillow; system python3 does."""
import json, os, sys
from PIL import Image, ImageDraw, ImageFont

frames, sheets = sys.argv[1], sys.argv[2]
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


BAND = 30
f_big, f_small = font(20), font(15)

for clip in index["clips"]:
    name, period, picks, views = clip["name"], clip["period"], clip["frames"], clip["views"]
    rows = []
    for view in views:
        tiles = [Image.open(os.path.join(frames, f"{name}_{view}_{i:02d}.png")).convert("RGB")
                 for i in range(len(picks))]
        rows.append((view, tiles))
    w, h = rows[0][1][0].size
    cols = len(picks)
    sheet = Image.new("RGB", (w * cols, (h + BAND) * len(rows) + BAND), (22, 22, 24))
    d = ImageDraw.Draw(sheet)
    d.text((10, 7), f"{name}   {period}-frame loop   ({period/24:.2f} s at 24 fps)",
           fill=(240, 240, 244), font=f_big)
    for r, (view, tiles) in enumerate(rows):
        top = BAND + r * (h + BAND)
        d.text((10, top + 6), f"{view}", fill=(190, 190, 200), font=f_small)
        for c, im in enumerate(tiles):
            sheet.paste(im, (c * w, top + BAND))
            # phase label, so a pose can be pointed at
            d.text((c * w + 8, top + BAND + 6), f"{picks[c]}  ({c / cols:.2f})",
                   fill=(230, 230, 235), font=f_small)
    out = os.path.join(sheets, f"work_{name}.png")
    sheet.save(out)
    print(f"  {os.path.basename(out):18s} {cols}x{len(rows)}  {os.path.getsize(out)/1024:.0f} KB")
