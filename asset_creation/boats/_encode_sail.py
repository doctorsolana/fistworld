"""Compose wind-fill and sail-direction frames into one labelled sheet. Needs Pillow."""

import os
import sys

from PIL import Image, ImageDraw, ImageFont


frames, output = sys.argv[1:3]
entries = []
with open(os.path.join(frames, "index.txt"), encoding="utf-8") as index:
    for line in index:
        line = line.strip()
        if line:
            entries.append(tuple(line.split("|", 1)))


def font(size, bold=False):
    candidates = (["/System/Library/Fonts/Supplemental/Arial Bold.ttf"] if bold else [])
    candidates += ["/System/Library/Fonts/Helvetica.ttc"]
    for path in candidates:
        try:
            return ImageFont.truetype(path, size)
        except OSError:
            pass
    return ImageFont.load_default()


images = [(caption, Image.open(os.path.join(frames, f"{name}.png")).convert("RGB"))
          for name, caption in entries]
width, height = images[0][1].size
columns, gap, title, caption_height = 3, 5, 54, 34
rows = (len(images) + columns - 1) // columns
sheet = Image.new("RGB", (columns * (width + gap), title + rows * (height + caption_height + gap)),
                  (20, 22, 25))
draw = ImageDraw.Draw(sheet)
draw.text((16, 13), "Dinghy sail — continuous wind fill and wind-direction pivot",
          fill=(245, 245, 248), font=font(25, True))
for index, (caption, image) in enumerate(images):
    x = (index % columns) * (width + gap)
    y = title + (index // columns) * (height + caption_height + gap)
    sheet.paste(image, (x, y))
    draw.text((x + 10, y + height + 5), caption, fill=(235, 236, 241), font=font(20, True))
sheet.save(output)
print(f"{os.path.basename(output)}: {len(images)} states, {os.path.getsize(output)/1024:.0f} KB")

