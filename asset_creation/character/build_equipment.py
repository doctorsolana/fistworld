"""Armour and headgear; called by the wardrobe build before manifest emission."""

import itertools

import bpy
from equipment_mesh import Wearable

IRON = (0.19, 0.24, 0.28)
IRON_DARK = (0.095, 0.12, 0.145)
BRASS = (0.38, 0.24, 0.075)
LEATHER = (0.13, 0.065, 0.029)
STRAP = (0.055, 0.026, 0.012)
WOOL = (0.07, 0.095, 0.12)
PADDED = (0.30, 0.205, 0.10)


def build_equipment(collection, rig):
    for name, base, trim in [
        ("Top_PaddedArmour", PADDED, LEATHER),
        ("Top_LeatherArmour", LEATHER, BRASS),
        ("Top_MailArmour", IRON, IRON_DARK),
    ]:
        w = Wearable(name)
        w.box((-0.178, -0.068, 0.265), (0.178, 0.127, 0.619), "torso", base)
        w.box((-0.184, -0.078, 0.278), (0.184, 0.137, 0.31), "torso", STRAP)
        w.box((-0.028, -0.088, 0.279), (0.028, -0.075, 0.31), "torso", BRASS)
        for sign, side in [(1, "L"), (-1, "R")]:

            def box(x0, y0, z0, x1, y1, z1, bone, colour, sign=sign, w=w):
                w.box(
                    (min(sign * x0, sign * x1), y0, z0),
                    (max(sign * x0, sign * x1), y1, z1),
                    bone,
                    colour,
                )

            box(0.132, -0.07, 0.45, 0.267, 0.119, 0.616, "arm." + side, base)
            if name != "Top_LeatherArmour":
                box(0.154, -0.068, 0.345, 0.301, 0.117, 0.46, "forearm." + side, base)
            box(0.15, -0.073, 0.48, 0.269, 0.122, 0.516, "arm." + side, trim)
            # Split coat tails ride the thighs rather than intersecting their cuffs.
            box(0.005, -0.070, 0.198, 0.174, 0.129, 0.305, "leg." + side, base)
        if name == "Top_PaddedArmour":
            for x in [-0.12, -0.06, 0, 0.06, 0.12]:
                w.box(
                    (x - 0.004, -0.073, 0.321),
                    (x + 0.004, -0.068, 0.585),
                    "torso",
                    (0.215, 0.14, 0.066),
                    0,
                )
            w.box((-0.035, -0.079, 0.54), (0.035, -0.067, 0.62), "torso", LEATHER)
        elif name == "Top_LeatherArmour":
            w.box(
                (-0.11, -0.078, 0.355),
                (0.11, -0.068, 0.56),
                "torso",
                (0.20, 0.105, 0.045),
            )
            for x in [-0.143, 0.143]:
                for z in [0.35, 0.42, 0.49, 0.56]:
                    w.box(
                        (x - 0.006, -0.083, z - 0.006),
                        (x + 0.006, -0.075, z + 0.006),
                        "torso",
                        BRASS,
                        0,
                    )
        else:
            # Sparse linked rows suggest mail at RTS distance without individual rings.
            for row in range(7):
                for col in range(5):
                    x = -0.14 + col * 0.063 + (row % 2) * 0.012
                    z = 0.335 + row * 0.036
                    w.box(
                        (x, -0.075, z),
                        (x + 0.029, -0.069, z + 0.006),
                        "torso",
                        (0.30, 0.35, 0.38),
                        0,
                    )
        w.finish(collection, rig)

    w = Wearable("Bottom_WoolBoots")
    w.box((-0.164, -0.052, 0.26), (0.164, 0.11, 0.415), "torso", WOOL)
    for sign, side in [(1, "L"), (-1, "R")]:

        def box(x0, y0, z0, x1, y1, z1, bone, c, sign=sign):
            w.box(
                (min(sign * x0, sign * x1), y0, z0),
                (max(sign * x0, sign * x1), y1, z1),
                bone,
                c,
            )

        box(0.01, -0.051, 0.085, 0.163, 0.110, 0.30, "leg." + side, WOOL)
        box(0.009, -0.062, 0.072, 0.167, 0.119, 0.16, "leg." + side, LEATHER)
        # Foot coverage extends just under grade; ankle cuff overlaps the foot shell.
        box(0.004, -0.121, -0.002, 0.170, 0.123, 0.09, "foot." + side, LEATHER)
        box(0.002, -0.123, -0.002, 0.172, 0.125, 0.014, "foot." + side, STRAP)
    w.finish(collection, rig)

    # Headgear is independent of hairstyle. Coverage metadata hides hair only
    # while a covering item is selected; the stored hairstyle stays unchanged.
    old = bpy.data.objects.get("Headgear_None")
    if old:
        bpy.data.objects.remove(old, do_unlink=True)
    empty = bpy.data.objects.new("Headgear_None", None)
    collection.objects.link(empty)
    empty.parent = rig
    for name, nasal in [("Headgear_NasalHelmet", True), ("Headgear_IronCap", False)]:
        w = Wearable(name)
        w.dome(
            [
                (0.218, 0.197, 0.869),
                (0.224, 0.205, 0.975),
                (0.214, 0.200, 1.031),
                (0.075, 0.070, 1.10),
            ],
            "head",
            IRON,
        )
        w.dome([(0.224, 0.203, 0.868), (0.224, 0.203, 0.897)], "head", IRON_DARK)
        crest = [
            (0, -0.176, 0.892),
            (0, -0.177, 1.034),
            (0, -0.044, 1.107),
            (0, 0.100, 1.107),
            (0, 0.234, 1.034),
            (0, 0.232, 0.892),
        ]
        for a, b in itertools.pairwise(crest):
            w.beam(a, b, 0.027, 0.014, "head", BRASS)
        if nasal:
            w.box(
                (-0.020, -0.187, 0.771),
                (0.020, -0.173, 0.902),
                "head",
                IRON_DARK,
                0.003,
            )
            w.box((-0.011, -0.191, 0.782), (0.011, -0.186, 0.901), "head", BRASS, 0.002)
        for x in [-0.143, -0.072, 0.072, 0.143]:
            w.box(
                (x - 0.005, -0.181, 0.879),
                (x + 0.005, -0.175, 0.889),
                "head",
                BRASS,
                0.002,
            )
        w.finish(collection, rig)
