"""Wardrobe DATA. No Blender imports -- pure numbers, so adding an item never means reading the
builder.

Adding an item is one block here plus nothing else. `build_wardrobe_v2.py` walks these lists and
`preview_wardrobe_v2.py` reads ITEMS for its catalogue, so a new entry appears in both automatically.

Slot prefixes are load-bearing: the game enumerates wardrobe by prefix, and only ONE item per slot
may be visible at a time (section 8 -- overlapping garments make one show through another and read
as an untextured patch).

Everything is fitted to these MEASURED body bounds. Change the body and every number here is stale:
    torso   x +-0.1456  y -0.0381..+0.0947  z 0.2600..0.6200   (hip joint at 0.26)
    leg.L   x  0.0206..0.1497                z 0.0800..0.2950
    arm.L   x  0.1396..0.2900                z 0.2997..0.6035   (wrist 0.2997; the arm SLANTS)
    head    x +-0.1904  y +-0.1475           z 0.6200..0.9980   (face plane y -0.1475)
    ear.L   x  0.1865..0.2549                z 0.6973..0.8125
    eyes                                     top z 0.8446
"""

# --- palettes (linear) ------------------------------------------------------------------------------
CLOTH = {
    "Brown": (0.1470, 0.0648, 0.0319),
    "Olive": (0.0931, 0.0976, 0.0437),
    "Red":   (0.4851, 0.0802, 0.0452),
    "Navy":  (0.0203, 0.0356, 0.1070),
    # Added for the second wave. Chosen to occupy gaps in the existing four rather than to be
    # "more colours": a cool neutral, a light warm, a mid earth-orange, and a desaturated green
    # well clear of Olive's yellow cast.
    "Slate": (0.0400, 0.0455, 0.0560),
    "Sand":  (0.2600, 0.1900, 0.0900),
    "Rust":  (0.2300, 0.0700, 0.0250),
    "Moss":  (0.0430, 0.0730, 0.0330),
}

# Spread the DARK end hard. A first palette bottomed out at linear 0.140 and still rendered as a
# medium tan: under a 300 W key with the PBR Neutral transform, mid albedo reads bright.
SKIN_TONES = {
    "Porcelain": (0.8500, 0.5450, 0.3950),
    "Fair":      (0.8600, 0.4700, 0.2900),
    "Tan":       (0.8070, 0.3372, 0.1170),   # the original v1/v2 skin
    "Olive":     (0.4300, 0.2050, 0.0780),
    "Brown":     (0.1800, 0.0780, 0.0320),
    "Deep":      (0.0620, 0.0260, 0.0130),
}
DEFAULT_SKIN = "Tan"

BROWN = [(0.0685, 0.0273, 0.0116), (0.0908, 0.0356, 0.0144),
         (0.1119, 0.0452, 0.0194), (0.1441, 0.0612, 0.0296)]
SAND = [(0.2050, 0.1330, 0.0480), (0.2600, 0.1750, 0.0620),
        (0.3150, 0.2200, 0.0850), (0.3700, 0.2700, 0.1150)]
BLACK = [(0.0180, 0.0150, 0.0140), (0.0300, 0.0260, 0.0240),
         (0.0450, 0.0390, 0.0360), (0.0640, 0.0560, 0.0520)]
AUBURN = [(0.0900, 0.0250, 0.0110), (0.1250, 0.0380, 0.0170),
          (0.1650, 0.0550, 0.0260), (0.2150, 0.0800, 0.0400)]

# --- bottoms: (name, hem z, cloth colour, cuff) ------------------------------------------------------
# Hip piece rides `torso`; thigh pieces ride the legs and run UP past the hip plane to 0.30, where the
# wider hip piece hides them -- the same overlap trick the body's own hip stub uses.
#
# The leg runs z 0.0800..0.2950, so hem z IS the cut: 0.195 above the knee down to 0.085 at the ankle.
# `cuff` adds a turned-up band standing proud at the hem -- section 7's "vary the cut, not just the
# colour" for 8 verts a leg.
# ORDER IS A WIRE CONTRACT -- APPEND ONLY, never insert or reorder.
# An outfit is replicated and stored as a u8 INDEX per slot (shared/src/components/actors.rs), and
# CharacterSlot::item() resolves it positionally. These lists were first written shortest-to-longest,
# which put the two new bottoms at indices 1 and 3 and silently redressed every existing villager
# holding index 1. Reading in hem order is worth nothing next to that.
BOTTOMS = [
    ("Bottom_Shorts",      0.1950, "Brown", False),   # 0  above the knee
    ("Bottom_Shorts_Long", 0.1180, "Olive", False),   # 1  calf
    ("Bottom_Breeches",    0.1550, "Sand",  True),    # 2  below the knee, cuffed
    ("Bottom_Trousers",    0.0850, "Slate", False),   # 3  ankle; leg bottoms out at 0.0800
]

# --- tops: (name, sleeve length, cloth colour, hem z, skirt) -----------------------------------------------
# "long" needs two segments per arm: the arm slants (x 0.1396..0.2157 at the shoulder against
# 0.1764..0.2900 at the wrist), so one box sized off its bounding box reads as a shoulder pad.
# "none" is a sleeveless jerkin -- bare arms are the entire silhouette difference, which is a bigger
# read than any colour change.
#
# hem z must stay ABOVE the hip joint at 0.26: the body piece binds rigidly to `torso`, so a hem
# below the joint would swing with the chest while the legs rotate under it.
#
# `skirt` adds a separate flared piece that DOES hang below the joint, which is what actually makes a
# tunic read as one. Dropping the body hem alone from 0.30 to 0.27 is 3 cm on a 1.7 m character and
# is invisible next to a tee. The skirt is still on `torso` -- correct, since a tunic hem hangs from
# the body rather than following the leg -- and is cut wide enough that a swinging thigh clears it.
# APPEND ONLY -- see the note on BOTTOMS.
TOPS = [
    ("Top_Tee",        "short", "Red",   0.3000, False),   # 0
    ("Top_LongSleeve", "long",  "Navy",  0.3000, False),   # 1
    ("Top_Jerkin",     "none",  "Rust",  0.3000, False),   # 2  sleeveless; bare arms are the silhouette
    ("Top_Tunic",      "short", "Moss",  0.2700, True),    # 3  hangs past the hip
]

# --- hair: (name, slabs, tones, noise cell) ---------------------------------------------------------
# Slabs must sit PROUD of the skull (head is x +-0.1904) or the head's own silhouette hides them and
# the result reads as a cap -- or, with two equal bands at the crown, a bandana. They must also
# OVERLAP in z; meeting edge-to-edge, each box's chamfer draws a seam line across the head.
# Hard limits: nothing below z 0.8446 where it reaches forward of the face plane (it would hang over
# the eyes), and nothing inside x 0.1865..0.2549 below z 0.8125 (it would intersect an ear).
HAIR = [
    ("Hair_Tousled", [
        ((-0.2050, -0.1600, 0.9250), (0.2050, 0.1600, 1.0080)),     # crown
        ((-0.1940, -0.1740, 0.9560), (0.1940, 0.0400, 1.0190)),     # top layer, forward + up
        ((-0.2050, -0.1860, 0.8680), (0.2050, -0.0900, 0.9310)),    # fringe, OVERHANGS to y -0.186
        ((-0.2080, 0.0450, 0.8180), (0.2080, 0.1660, 0.9310)),      # back of the skull
        ((0.1520, -0.0900, 0.8250), (0.2080, 0.0450, 0.9310)),      # side L
        ((-0.2080, -0.0900, 0.8250), (-0.1520, 0.0450, 0.9310)),    # side R
        ((0.1600, -0.1500, 0.8160), (0.2080, -0.0600, 0.8800)),     # sideburn L, front of the ear
        ((-0.2080, -0.1500, 0.8160), (-0.1600, -0.0600, 0.8800)),   # sideburn R
        ((-0.1560, 0.1050, 0.7880), (0.1560, 0.1720, 0.8620)),      # nape, behind the ears
    ], BROWN, 0.045),

    ("Hair_Crop", [
        ((-0.2050, -0.1690, 0.8820), (0.2050, 0.1600, 1.0070)),     # solid mass incl. fringe
        ((-0.2080, -0.0450, 0.8150), (0.2080, 0.1640, 0.8900)),     # sides + back
        ((-0.2080, -0.1690, 0.8380), (-0.1500, -0.0450, 0.8900)),   # temple R, frames the face
        ((0.1500, -0.1690, 0.8380), (0.2080, -0.0450, 0.8900)),     # temple L
    ], BLACK, 0.045),

    ("Hair_Bowl", [
        ((-0.2080, -0.1650, 0.8900), (0.2080, 0.1650, 1.0130)),
        ((-0.2160, -0.1740, 0.8600), (0.2160, 0.1740, 0.9000)),     # flared brim
    ], SAND, 0.036),

    # spikes must not touch each other, or build()'s one-shell-per-box assert fires
    ("Hair_Spiky", [
        ((-0.2050, -0.1620, 0.9150), (0.2050, 0.1620, 0.9900)),     # base
        ((-0.2050, -0.1780, 0.8780), (0.2050, -0.1000, 0.9300)),    # fringe
        ((0.1520, -0.1000, 0.8400), (0.2080, 0.0600, 0.9300)),      # side L
        ((-0.2080, -0.1000, 0.8400), (-0.1520, 0.0600, 0.9300)),    # side R
        ((-0.1520, -0.1000, 0.9800), (-0.0720, 0.0000, 1.0620)),
        ((-0.0400, -0.1320, 0.9800), (0.0400, 0.0120, 1.0880)),
        ((0.0720, -0.1000, 0.9800), (0.1520, 0.0000, 1.0620)),
        ((-0.1120, 0.0300, 0.9800), (-0.0220, 0.1300, 1.0460)),
        ((0.0220, 0.0300, 0.9800), (0.1120, 0.1300, 1.0460)),
    ], BLACK, 0.045),

    ("Hair_Long", [
        ((-0.2050, -0.1600, 0.9250), (0.2050, 0.1600, 1.0080)),     # crown
        ((-0.2050, -0.1840, 0.8700), (0.2050, -0.0900, 0.9310)),    # fringe
        ((0.1520, -0.0900, 0.8300), (0.2080, 0.0450, 0.9310)),      # side L, stops above the ear
        ((-0.2080, -0.0900, 0.8300), (-0.1520, 0.0450, 0.9310)),    # side R
        ((-0.2050, 0.0500, 0.6900), (0.2050, 0.1720, 0.9310)),      # back curtain, clears the ears
        ((-0.1760, 0.0820, 0.6500), (0.1760, 0.1660, 0.6980)),      # tapered tip
    ], AUBURN, 0.045),

    # A stepped dome, the widest style here. The low sections stop at y -0.06 so the mass never
    # reaches forward of the face below the eye line; the nape drops to 0.74 only because it stays
    # behind y 0.08, clear of the ears. Cell 0.036 rather than 0.045 reads curlier (section 8).
    ("Hair_Afro", [
        ((-0.2650, -0.2350, 0.8750), (0.2650, 0.2450, 0.9950)),     # main mass
        ((-0.2350, -0.2050, 0.9900), (0.2350, 0.2150, 1.0550)),     # upper dome
        ((-0.1750, -0.1450, 1.0500), (0.1750, 0.1550, 1.0950)),     # crown cap
        ((-0.2450, -0.0600, 0.8150), (0.2450, 0.2350, 0.8850)),     # lower sides + back
        ((-0.2000, 0.0800, 0.7400), (0.2000, 0.2350, 0.8250)),      # nape
    ], BLACK, 0.036),
]

# Slot -> the items in it. Only one per slot may be visible; the game enumerates by these lists.
SLOTS = {
    "bottom": [n for n, *_ in BOTTOMS],
    "top": [n for n, *_ in TOPS],
    "hair": [n for n, *_ in HAIR],
}
DEFAULT_OUTFIT = {"bottom": "Bottom_Shorts", "top": "Top_Tee", "hair": "Hair_Tousled"}
ITEMS = SLOTS["bottom"] + SLOTS["top"] + SLOTS["hair"]
