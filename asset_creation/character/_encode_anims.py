"""Compose the per-clip frames into one contact sheet. Needs Pillow."""
import json
import os
import sys

from PIL import Image, ImageDraw, ImageFont

frames, sheets = sys.argv[1], sys.argv[2]
index = json.load(open(os.path.join(frames, "index.json")))


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


rows = index["rows"]
first = Image.open(os.path.join(frames, "r00_0.png"))
TW, TH = first.size
LABEL, TITLE, GAP = 210, 46, 4
cols = len(rows[0]["frames"])

W = LABEL + cols * (TW + GAP)
H = TITLE + len(rows) * (TH + GAP)
sheet = Image.new("RGB", (W, H), (22, 22, 25))
d = ImageDraw.Draw(sheet)
d.text((14, 12), "every clip with the item it holds   4 evenly spaced frames per clip",
       fill=(242, 242, 248), font=font(24, True))

for r, row in enumerate(rows):
    y = TITLE + r * (TH + GAP)
    if r % 2:
        d.rectangle([0, y, W, y + TH + GAP - 1], fill=(30, 30, 34))
    d.text((16, y + TH // 2 - 26), row["label"], fill=(246, 246, 252), font=font(22, True))
    d.text((16, y + TH // 2 + 2), f"{row['period']} frames", fill=(150, 150, 162), font=font(18))
    for c, f in enumerate(row["frames"]):
        p = os.path.join(frames, f"r{r:02d}_{c}.png")
        if not os.path.exists(p):
            continue
        x = LABEL + c * (TW + GAP)
        sheet.paste(Image.open(p).convert("RGB"), (x, y))
        d.text((x + 8, y + 8), f"f{f}", fill=(190, 190, 205), font=font(17))

out = os.path.join(sheets, "animations_with_items.png")
sheet.save(out)
print(f"  {os.path.basename(out):26s} {len(rows)} rows x {cols}  {os.path.getsize(out)/1024:.0f} KB")

tools = index.get("tools", [])
if tools:
    views = tools[0]["views"]
    W2 = LABEL + len(views) * (TW + GAP)
    H2 = TITLE + 26 + len(tools) * (TH + GAP)
    s2 = Image.new("RGB", (W2, H2), (22, 22, 25))
    d2 = ImageDraw.Draw(s2)
    d2.text((14, 12), "tool facing check   neutral pose, four azimuths",
            fill=(242, 242, 248), font=font(24, True))
    d2.text((14, 42), "the character faces the camera at 'front'; a blade or hammer face must "
                      "point the same way",
            fill=(158, 158, 172), font=font(18))
    for r, t in enumerate(tools):
        y = TITLE + 26 + r * (TH + GAP)
        if r % 2:
            d2.rectangle([0, y, W2, y + TH + GAP - 1], fill=(30, 30, 34))
        d2.text((16, y + TH // 2 - 12), t["label"], fill=(246, 246, 252), font=font(22, True))
        for c, nm in enumerate(views):
            p = os.path.join(frames, f"t{r:02d}_{c}.png")
            if not os.path.exists(p):
                continue
            x = LABEL + c * (TW + GAP)
            s2.paste(Image.open(p).convert("RGB"), (x, y))
            d2.text((x + 8, y + 8), nm, fill=(190, 190, 205), font=font(17))
    out2 = os.path.join(sheets, "tool_facing.png")
    s2.save(out2)
    print(f"  {os.path.basename(out2):26s} {len(tools)} tools x {len(views)}  "
          f"{os.path.getsize(out2)/1024:.0f} KB")
