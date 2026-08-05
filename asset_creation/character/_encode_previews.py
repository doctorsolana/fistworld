"""Encode rendered frames into looping WebPs and contact sheets.

Called by preview_basemodel_v2.py as a subprocess, because Blender's bundled Python has no Pillow
while the system python3 does (section 9).

    python3 _encode_previews.py <frames_dir> <anim_out> <sheets_out>
"""

import glob
import os
import re
import sys

from PIL import Image

frames_dir, anim_out, sheets_out = sys.argv[1:4]
FPS = 24

# --- one webp per clip -----------------------------------------------------------------------------
clips = {}
for path in sorted(glob.glob(os.path.join(frames_dir, "*_[0-9][0-9][0-9].png"))):
    name = re.sub(r"_\d{3}\.png$", "", os.path.basename(path))
    clips.setdefault(name, []).append(path)

for name, paths in sorted(clips.items()):
    frames = [Image.open(p).convert("RGB") for p in sorted(paths)]
    out = os.path.join(anim_out, f"{name}.webp")
    frames[0].save(out, save_all=True, append_images=frames[1:],
                   duration=int(1000 / FPS), loop=0, quality=86, method=4)
    print(f"  {os.path.basename(out):24s} {len(frames):3d} frames  {os.path.getsize(out)/1024:6.0f} KB")


def sheet(paths, out, cols):
    ims = [Image.open(p).convert("RGB") for p in paths]
    if not ims:
        return
    w, h = ims[0].size
    rows = (len(ims) + cols - 1) // cols
    s = Image.new("RGB", (w * cols, h * rows), (24, 24, 26))
    for i, im in enumerate(ims):
        s.paste(im, ((i % cols) * w, (i // cols) * h))
    s.save(out)
    print(f"  {os.path.basename(out):24s} {len(ims)} tiles  {os.path.getsize(out)/1024:6.0f} KB")


MOODS = ("face_idle", "face_happy", "face_angry", "face_sad", "face_surprised")
sheet([os.path.join(frames_dir, f"mood_{m}.png") for m in MOODS
       if os.path.exists(os.path.join(frames_dir, f"mood_{m}.png"))],
      os.path.join(sheets_out, "moods.png"), 5)

TURN = ("front", "34", "side", "back")
sheet([os.path.join(frames_dir, f"turn_{t}.png") for t in TURN
       if os.path.exists(os.path.join(frames_dir, f"turn_{t}.png"))],
      os.path.join(sheets_out, "turnaround.png"), 4)
