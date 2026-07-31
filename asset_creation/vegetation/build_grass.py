"""Build a grass PATCH — crossed alpha cards, Valheim style — plus the little texture it needs.

    blender --background --factory-startup --python asset_creation/vegetation/build_grass.py -- [--seed 1]

    # live, in the Blender MCP session
    import sys; sys.argv = ['x', '--', '--seed', '1']
    exec(open('/Users/terminator2/Coding/fistworld/asset_creation/vegetation/build_grass.py').read())

ONE ENTITY PER PATCH, NEVER PER BLADE. That is the whole design. `Env_Grass_Tall_04` is in the map
38,578 times at 738 triangles; if it ever drew, that would be 28.5M triangles, more than every tree
combined. It does not draw -- the client classes it GroundDetail and the spawn filter skips every
one -- so today it costs nothing and contributes nothing.

A patch is ~2 x 2 m of ground carrying a handful of crossed cards, about 40 triangles, standing in
for what would otherwise be forty individual blades.

GRASS IS THE ONE PLACE ALPHA IS RIGHT. The vegetation contract says opaque, and for trees it is
correct -- geometry does that job. A blade of grass is 3 mm wide; there is no geometry small enough,
and the shape has to come from the alpha channel. The pines taught the same lesson from the other
direction: 71% of their leaf texture is cut away, and no triangle budget reproduces it.

The texture is deliberately TINY. A 1024x1024 PNG of flat colour compresses to nothing on disk and
is still 4 MB uncompressed on the GPU -- which is how the existing environment set reaches ~880 MB
of VRAM from 7 MB of PNG. At 256x256 this is 256 KB. Size is the thing that matters, not count.

Cards are CROSSED, not single. A lone card vanishes when the camera lines up with its plane; two at
right angles always present something. And at the RTS camera's 40.8 deg above the horizon a
vertical card still shows ~65% of its area, so they do not need to lean the way wheat straws did.
"""

import math
import os
import random
import struct
import sys
import zlib

import bpy
import bmesh
from mathutils import Vector

ARGV = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []


def arg(flag, default):
    return ARGV[ARGV.index(flag) + 1] if flag in ARGV else default


def log(m):
    print(f"[grass] {m}", flush=True)


# Variants mix on the ground: short/dark reads as turf, tall/bright as meadow. Both are the same
# 2 m patch with the same card count, so the cost is identical and only the look changes.
VARIANTS = {
    "short": dict(blade_w=(0.34, 0.52), blade_h=(0.28, 0.46), cards=(9, 3),
                  greens=[(74, 112, 41), (92, 132, 48), (110, 148, 58), (62, 96, 36)],
                  taper=0.85, bend=0.30),
    "tall":  dict(blade_w=(0.40, 0.62), blade_h=(0.62, 0.95), cards=(9, 3),
                  greens=[(126, 168, 62), (146, 186, 74), (112, 154, 54), (160, 196, 88)],
                  taper=1.15, bend=0.44),
}

VARIANT = arg("--variant", "short")
SEED = int(arg("--seed", "1"))
NAME = arg("--name", f"Grass_Patch_{VARIANT.capitalize()}_A")
V = VARIANTS[VARIANT]
REPO = "/Users/terminator2/Coding/fistworld/asset_creation"
OUT = arg("--out", os.path.join(REPO, "vegetation"))
TEX = os.path.join(OUT, f"Grass_Blades_{arg('--variant', 'short')}.png")

TEX_SIZE = 256          # 256 KB of VRAM. A 1024 would be 4 MB, for no visible gain at this scale.
PATCH = 2.0             # metres square
CARDS = V["cards"]      # crossed pairs per LOD
BLADE_W = V["blade_w"]
BLADE_H = V["blade_h"]
CUTOFF = 0.5            # matches AlphaMode::Mask(0.5) in client/src/props/foliage.rs

rng = random.Random(SEED)


def write_png(path, pixels, w, h):
    """Minimal RGBA PNG writer — no PIL inside Blender."""
    raw = b"".join(b"\x00" + bytes(pixels[y * w * 4:(y + 1) * w * 4]) for y in range(h))

    def chunk(tag, data):
        c = tag + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c) & 0xFFFFFFFF)

    png = (b"\x89PNG\r\n\x1a\n"
           + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(raw, 9))
           + chunk(b"IEND", b""))
    with open(path, "wb") as fh:
        fh.write(png)


def make_texture(path):
    """A tuft of blades, alpha-cut, drawn straight into a pixel buffer.

    Blades taper to a point and curve, because a straight-sided rectangle reads as a fence post at
    any distance. The alpha is hard-edged: the material is Mask, not Blend, so soft edges would
    just pop at the cutoff rather than fade.
    """
    w = h = TEX_SIZE
    px = bytearray(w * h * 4)                       # transparent
    jitter = random.Random(SEED * 7717)
    greens = V["greens"]

    for _ in range(26):
        base_x = jitter.uniform(0.06, 0.94) * w
        base_y = h - 1
        height = jitter.uniform(0.42, 0.92) * h * (1.0 if VARIANT == "short" else 1.06)
        width = jitter.uniform(0.014, 0.030) * w
        bend = jitter.uniform(-V["bend"], V["bend"]) * w
        r, g, b = greens[jitter.randrange(len(greens))]
        steps = int(height)
        for s in range(steps):
            t = s / max(steps - 1, 1)
            y = int(base_y - t * height)
            if not (0 <= y < h):
                continue
            cx = base_x + bend * (t ** 1.7)
            half = max(0.5, width * (1.0 - t ** 0.85))     # taper to a tip
            shade = 0.72 + 0.28 * t                        # lighter toward the tip
            for x in range(int(cx - half), int(cx + half) + 1):
                if not (0 <= x < w):
                    continue
                i = (y * w + x) * 4
                px[i] = int(r * shade)
                px[i + 1] = int(g * shade)
                px[i + 2] = int(b * shade)
                px[i + 3] = 255
    write_png(path, px, w, h)
    return w, h


def card(bm, uv_layer, centre, width, height, angle):
    """One upright quad, UV-mapped to the whole texture."""
    right = Vector((math.cos(angle), math.sin(angle), 0.0)) * (width * 0.5)
    corners = [centre - right,
               centre + right,
               centre + right + Vector((0, 0, height)),
               centre - right + Vector((0, 0, height))]
    verts = [bm.verts.new(c) for c in corners]
    face = bm.faces.new(verts)
    for loop, uv in zip(face.loops, [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]):
        loop[uv_layer].uv = uv
    return face


def main():
    sc = bpy.data.scenes.get("Grass") or bpy.data.scenes.new("Grass")
    if bpy.context.window:
        bpy.context.window.scene = sc
    for o in list(sc.objects):
        bpy.data.objects.remove(o, do_unlink=True)
    os.makedirs(OUT, exist_ok=True)

    tw, th = make_texture(TEX)
    log(f"texture {tw}x{th} -> {TEX} ({os.path.getsize(TEX) / 1024:.0f} KB on disk, "
        f"{tw * th * 4 / 1024:.0f} KB in VRAM)")

    image = bpy.data.images.load(TEX, check_existing=True)
    image.name = f"Grass_Blades_{VARIANT}"
    mat = bpy.data.materials.new(f"grass_cutout_{VARIANT}")
    mat.use_nodes = True
    nt = mat.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = image
    tex.interpolation = "Linear"
    nt.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    nt.links.new(tex.outputs["Alpha"], bsdf.inputs["Alpha"])
    bsdf.inputs["Metallic"].default_value = 0.0
    bsdf.inputs["Roughness"].default_value = 0.95
    # MASK, not BLEND: cutout needs no sorting and keeps depth writes, which matters when thousands
    # of these overlap. Blend would force back-to-front sorting per patch.
    mat.blend_method = "CLIP" if hasattr(mat, "blend_method") else mat.blend_method
    mat.use_backface_culling = False              # a card must be visible from both sides

    built = []
    for level, n_cards in enumerate(CARDS):
        rng.seed(SEED)
        bm = bmesh.new()
        uv_layer = bm.loops.layers.uv.new("UVMap")
        for _ in range(n_cards):
            pos = Vector((rng.uniform(-0.5, 0.5) * PATCH, rng.uniform(-0.5, 0.5) * PATCH, 0.0))
            w = rng.uniform(*BLADE_W)
            hgt = rng.uniform(*BLADE_H)
            a = rng.uniform(0, math.pi)
            card(bm, uv_layer, pos, w, hgt, a)                 # crossed pair: a and a+90 deg
            card(bm, uv_layer, pos, w, hgt, a + math.pi * 0.5)

        # Wind weight rises with height; the roots stay pinned. TEXCOORD_1, never COLOR_0.a --
        # bevy multiplies COLOR_0 into base colour and feeds alpha_discard, so a 0 weight at the
        # root would delete the root outright.
        wind = bm.loops.layers.uv.new("Wind")
        colour = bm.loops.layers.color.new("Color")
        for face in bm.faces:
            for loop in face.loops:
                z = loop.vert.co.z
                tint = 0.86 + 0.14 * min(1.0, z / max(BLADE_H[1], 1e-6))
                loop[colour] = (tint, tint, tint, 1.0)          # alpha ALWAYS 1.0
                loop[wind].uv = (min(1.0, z / max(BLADE_H[1], 1e-6)), 0.0)

        me = bpy.data.meshes.new(f"{NAME}_LOD{level}")
        bm.to_mesh(me)
        bm.free()
        obj = bpy.data.objects.new(me.name, me)
        sc.collection.objects.link(obj)
        me.materials.append(mat)
        me.calc_loop_triangles()
        built.append((obj, len(me.loop_triangles)))

    a, b = built[0][1], built[1][1]
    log(f"{NAME}: LOD0 {a} tris ({CARDS[0]} crossed cards), LOD1 {b} tris "
        f"({b / a * 100:.0f}%) over {PATCH} x {PATCH} m")

    for o in sc.objects:
        o.select_set(False)
    for o, _ in built:
        o.select_set(True)
    bpy.context.view_layer.objects.active = built[0][0]
    path = os.path.join(OUT, f"{NAME}.glb")
    bpy.ops.export_scene.gltf(
        filepath=path, export_format="GLB", use_selection=True,
        export_materials="EXPORT", export_yup=True, export_apply=True, export_attributes=True,
        use_active_scene=True,
    )
    log(f"wrote {path} ({os.path.getsize(path) / 1024:.0f} KB)")

    density = PATCH * PATCH
    for radius in (60, 80):
        patches = math.pi * radius * radius / density
        log(f"  at {radius} m draw radius: ~{patches:,.0f} patches = "
            f"{patches * b / 1e6:.2f}M tris at LOD1")


main()
