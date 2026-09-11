#!/usr/bin/env python3
"""Build the small, text-free exploration HUD artwork. Requires Pillow.

Run from any directory: python3 asset_creation/ui/build_hud.py
The source shapes are authored below, not extracted from a screenshot. Palette
values come from styles.rs. Panel edges are intended for Bevy nine-slicing;
icons remain separate transparent images so labels and tooltips stay native UI.
"""

from __future__ import annotations

import math
from pathlib import Path
import re

from PIL import Image, ImageChops, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "client/assets/ui/hud"
SCALE = 4
STYLES = (ROOT / "client/src/ui/styles.rs").read_text()


def palette(name):
    match = re.search(rf"pub const {name}: Color = Color::srgba?\(([^)]+)\)", STYLES)
    if not match:
        raise ValueError(f"Missing shared palette entry: {name}")
    return tuple(round(float(x.strip()) * 255) for x in match[1].split(",")[:3])


WOOD, GOLD, AGED, CREAM = (palette(n) for n in ("SIGN_WOOD", "BRASS", "BRASS_DARK", "PARCHMENT"))


def mix(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


class Drawing:
    """A tiny supersampled path canvas in logical pixels."""

    def __init__(self, width, height, mode="RGBA"):
        self.width, self.height = width, height
        self.image = Image.new(mode, (width * SCALE, height * SCALE), 0)
        self.draw = ImageDraw.Draw(self.image)

    def line(self, points, color, width=1):
        self.draw.line([(round(x * SCALE), round(y * SCALE)) for x, y in points],
                       fill=color, width=max(1, round(width * SCALE)), joint="curve")

    def polygon(self, points, color):
        self.draw.polygon([(round(x * SCALE), round(y * SCALE)) for x, y in points], fill=color)

    def ellipse(self, box, color, outline=None, width=1):
        self.draw.ellipse(tuple(round(v * SCALE) for v in box), fill=color,
                          outline=outline, width=max(1, round(width * SCALE)))

    def rounded(self, box, radius, color, outline=None, width=1):
        self.draw.rounded_rectangle(tuple(round(v * SCALE) for v in box),
                                    round(radius * SCALE), fill=color, outline=outline,
                                    width=max(1, round(width * SCALE)))

    def path(self, source, color=255):
        # Deliberately only the four commands used by these maintained paths.
        tokens = re.findall(r"[MLCZ]|-?\d+(?:\.\d+)?", source)
        points, cursor, i = [], (0.0, 0.0), 0
        while i < len(tokens):
            op = tokens[i]
            i += 1
            if op in "ML":
                cursor = tuple(map(float, tokens[i:i + 2]))
                i += 2
                points.append(cursor)
            elif op == "C":
                values = list(map(float, tokens[i:i + 6]))
                i += 6
                p0, p1, p2, p3 = cursor, values[:2], values[2:4], values[4:6]
                for step in range(1, 25):
                    t = step / 24
                    points.append(tuple((1 - t) ** 3 * p0[d] + 3 * (1 - t) ** 2 * t * p1[d]
                                        + 3 * (1 - t) * t * t * p2[d] + t ** 3 * p3[d]
                                        for d in range(2)))
                cursor = p3
            elif op == "Z":
                self.polygon(points, color)
                points = []
        if points:
            self.polygon(points, color)

    def save(self, name):
        self.image.resize((self.width, self.height), Image.Resampling.LANCZOS).save(OUT / name)


def grain(width, height):
    """Bounded deterministic horizontal walnut grain, quiet beneath lettering."""
    image = Image.new("RGBA", (width * SCALE, height * SCALE))
    pixels = image.load()
    base = mix(WOOD, CREAM, .045)
    for y in range(image.height):
        fy = y / SCALE
        for x in range(image.width):
            fx = x / SCALE
            wave = math.sin(fy * 2.3 + math.sin(fx * .034) * .5)
            fine = math.sin(fy * 7.8 + fx * .075)
            light = (1 - fy / height) * 4.0
            delta = wave * 1.2 + fine * .4 + light
            pixels[x, y] = tuple(max(0, min(255, round(c + delta))) for c in base) + (255,)
    return image


def panel(pill=False):
    width, height = (128, 48) if pill else (128, 96)
    radius = 23 if pill else 6
    canvas = Drawing(width, height)
    mask = Drawing(width, height, "L")
    mask.rounded((1, 1, width - 1, height - 1), radius, 255)
    texture = grain(width, height)
    texture.putalpha(mask.image)
    canvas.image.alpha_composite(texture)
    edge = mix(WOOD, (0, 0, 0), .45)
    canvas.rounded((.5, .5, width - .5, height - .5), radius + .5, None, edge, 1)
    canvas.rounded((1.5, 1.5, width - 1.5, height - 1.5), radius - .5, None, GOLD, 1.2)
    canvas.rounded((3.3, 3.3, width - 3.3, height - 3.3), radius - 2.3, None, AGED, .65)
    canvas.rounded((4.4, 4.4, width - 4.4, height - 4.4), radius - 3.4, None, edge, .65)
    # Thin glints and corner pins; no busy filigree behind copy.
    if not pill:
        for x, y, dx, dy in [(6, 6, 1, 1), (width - 6, 6, -1, 1),
                             (6, height - 6, 1, -1), (width - 6, height - 6, -1, -1)]:
            canvas.line([(x + dx * 6, y), (x + dx * 2, y), (x, y + dy * 2), (x, y + dy * 6)],
                        GOLD, .8)
            canvas.ellipse((x - .8, y - .8, x + .8, y + .8), mix(GOLD, CREAM, .32))
    canvas.save("pill.png" if pill else "panel.png")


def medallion():
    canvas = Drawing(128, 128)
    mask = Drawing(128, 128, "L")
    mask.ellipse((3, 3, 125, 125), 255)
    texture = grain(128, 128)
    texture.putalpha(mask.image)
    canvas.image.alpha_composite(texture)
    canvas.ellipse((1, 1, 127, 127), None, mix(WOOD, (0, 0, 0), .35), 2)
    canvas.ellipse((3, 3, 125, 125), None, AGED, 4)
    canvas.ellipse((5, 5, 123, 123), None, mix(GOLD, CREAM, .30), 1.8)
    canvas.ellipse((8.5, 8.5, 119.5, 119.5), None, GOLD, 1.5)
    canvas.ellipse((11, 11, 117, 117), None, mix(WOOD, (0, 0, 0), .45), 2)
    for x, y in [(64, 7), (121, 64), (64, 121), (7, 64)]:
        canvas.ellipse((x - 1.1, y - 1.1, x + 1.1, y + 1.1), mix(GOLD, CREAM, .38))
    canvas.save("medallion.png")


def shape(name):
    c = Drawing(64, 64, "L")
    if name == "crest":
        c.path("M32 4 C26 11 23 16 25 23 C27 27 29 32 29 39 L35 39 C35 32 37 27 39 23 C41 16 38 11 32 4 Z")
        c.path("M27 40 C26 25 16 19 10 24 C1 32 11 42 18 37 C12 39 10 31 14 31 C19 29 23 34 24 40 Z")
        c.path("M37 40 C38 25 48 19 54 24 C63 32 53 42 46 37 C52 39 54 31 50 31 C45 29 41 34 40 40 Z")
        c.rounded((21, 39, 43, 43), 1, 255)
        c.path("M27 44 L37 44 C36 51 34 55 32 60 C30 55 28 51 27 44 Z")
        c.path("M24 44 L28 44 C27 52 22 54 18 52 C23 51 25 48 24 44 Z")
        c.path("M40 44 L36 44 C37 52 42 54 46 52 C41 51 39 48 40 44 Z")
    elif name in ("purse", "bag"):
        c.path("M22 7 L28 10 L34 8 L43 7 L39 20 L25 20 Z")
        c.rounded((23, 21, 41, 24), 1, 255)
        c.path("M25 26 C17 32 12 40 13 51 C14 59 50 59 51 51 C52 40 47 32 39 26 Z")
        if name == "bag":
            c.line([(23, 30), (21, 47), (24, 53)], 0, 2)
            c.line([(35, 11), (32, 18)], 0, 1.5)
    elif name == "sun":
        c.ellipse((20, 20, 44, 44), 255)
        for i in range(8):
            a = i * math.tau / 8
            c.line([(32 + math.sin(a) * 20, 32 + math.cos(a) * 20),
                    (32 + math.sin(a) * 28, 32 + math.cos(a) * 28)], 255, 3)
    elif name == "moon":
        c.ellipse((9, 7, 55, 55), 255)
        c.ellipse((26, 2, 63, 41), 0)
        c.polygon([(49, 4), (51, 10), (57, 12), (51, 14), (49, 20), (47, 14), (41, 12), (47, 10)], 255)
    elif name == "bell":
        c.ellipse((28, 5, 36, 13), 255)
        c.path("M17 29 C17 17 22 12 32 12 C42 12 47 17 47 29 L48 42 L54 49 L54 52 L10 52 L10 49 L16 42 Z")
        c.ellipse((27, 50, 37, 60), 255)
        c.line([(19, 48), (45, 48)], 0, 1.5)
    elif name == "person":
        c.ellipse((23, 7, 41, 27), 255)
        c.path("M26 30 L38 30 C47 32 50 40 50 56 L14 56 C14 40 17 32 26 30 Z")
        c.line([(18, 52), (46, 52)], 0, 1.3)
    elif name == "scales":
        c.line([(32, 10), (32, 51)], 255, 3)
        c.line([(10, 18), (32, 13), (54, 18)], 255, 3)
        c.ellipse((28, 7, 36, 15), 255)
        for x, y in [(14, 19), (50, 19)]:
            c.line([(x, y), (x - 9, 38)], 255, 2)
            c.line([(x, y), (x + 9, 38)], 255, 2)
            c.path(f"M{x - 10} 38 L{x + 10} 38 C{x + 8} 49 {x - 8} 49 {x - 10} 38 Z")
        c.path("M29 49 L35 49 L39 54 L46 56 L46 59 L18 59 L18 56 L25 54 Z")
    elif name == "book":
        c.path("M5 12 C14 9 25 10 30 15 L30 53 C23 47 13 46 5 50 Z")
        c.path("M59 12 C50 9 39 10 34 15 L34 53 C41 47 51 46 59 50 Z")
        c.line([(3, 16), (3, 53), (13, 51), (23, 52), (30, 56), (34, 56), (41, 52), (51, 51), (61, 53), (61, 16)], 255, 2)
    elif name == "pin":
        c.path("M32 6 C8 6 11 31 21 43 L32 59 L43 43 C53 31 56 6 32 6 Z")
        c.ellipse((25, 15, 39, 29), 0)
    elif name == "heart":
        c.path("M32 17 C24 4 8 10 7 23 C6 34 17 44 32 57 C47 44 58 34 57 23 C56 10 40 4 32 17 Z")
    elif name == "chevron":
        c.line([(13, 40), (32, 21), (51, 40)], 255, 5)
    elif name == "compass":
        c.polygon([(32, 3), (38, 26), (61, 32), (38, 38), (32, 61), (26, 38), (3, 32), (26, 26)], 255)
        c.polygon([(32, 3), (32, 32), (38, 26)], 0)
        c.polygon([(61, 32), (32, 32), (38, 38)], 0)
        c.polygon([(32, 61), (32, 32), (26, 38)], 0)
        c.polygon([(3, 32), (32, 32), (26, 26)], 0)
        c.ellipse((28, 28, 36, 36), 255)
    else:
        raise ValueError(name)
    return c.image


def icon(name):
    mask = shape(name)
    canvas = Drawing(64, 64)
    # A narrow engraved edge retains the cream silhouette over lit wooden bars.
    shadow = Image.new("RGBA", mask.size, mix(AGED, WOOD, .55) + (255,))
    shadow.putalpha(mask.filter(ImageFilter.MaxFilter(5)))
    canvas.image.alpha_composite(shadow)
    fill = Image.new("RGBA", mask.size)
    draw = ImageDraw.Draw(fill)
    for y in range(fill.height):
        color = mix(GOLD, CREAM, .63 - .23 * y / fill.height)
        draw.line((0, y, fill.width, y), fill=color + (255,))
    fill.putalpha(mask)
    canvas.image.alpha_composite(fill)
    canvas.save(f"{name}.png")


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    panel()
    panel(pill=True)
    medallion()
    for name in ("crest", "purse", "sun", "moon", "bell", "person", "bag", "scales",
                 "book", "pin", "heart", "chevron", "compass"):
        icon(name)
    print(f"Wrote 16 reusable HUD images ({sum(p.stat().st_size for p in OUT.glob('*.png')):,} bytes) to {OUT}")


if __name__ == "__main__":
    main()
