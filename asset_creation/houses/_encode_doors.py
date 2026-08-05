"""Stack the door-lineup frames into one labelled sheet per clip. Blender has no Pillow; python3 does."""
import json, os, sys
from PIL import Image, ImageDraw, ImageFont

frames, sheets = sys.argv[1], sys.argv[2]
with open(os.path.join(frames, "index.json")) as fh:
    index = json.load(fh)


def font(size):
    for p in ("/System/Library/Fonts/Helvetica.ttc", "/System/Library/Fonts/Supplemental/Arial.ttf"):
        if os.path.exists(p):
            try:
                return ImageFont.truetype(p, size)
            except Exception:
                pass
    return ImageFont.load_default()


f_big, f_small = font(26), font(19)
BAND, TITLE = 30, 40

for clip in index["clips"]:
    name, picks, last = clip["name"], clip["frames"], clip["last"]
    tiles = [Image.open(os.path.join(frames, f"{name.split('_')[1]}_{i:02d}.png")).convert("RGB")
             for i in range(len(picks))]
    w, h = tiles[0].size
    sheet = Image.new("RGB", (w, TITLE + (h + BAND) * len(tiles)), (22, 22, 24))
    d = ImageDraw.Draw(sheet)
    d.text((14, 9), f"{name}   {last}-frame clip   ({last/24:.2f} s at 24 fps)   "
                    f"left to right: {'  /  '.join(index['labels'])}",
           fill=(242, 242, 246), font=f_big)
    for r, im in enumerate(tiles):
        top = TITLE + r * (h + BAND)
        d.text((14, top + 6), f"frame {picks[r]}   phase {r/(len(picks)-1):.2f}",
               fill=(198, 198, 208), font=f_small)
        sheet.paste(im, (0, top + BAND))
    out = os.path.join(sheets, f"{name}_lineup.png")
    sheet.save(out)
    print(f"  {os.path.basename(out):24s} {len(tiles)} rows  {os.path.getsize(out)/1024:.0f} KB")
