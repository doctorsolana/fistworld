"""Dedicated baseline sidearm poses, using the canonical rig's measured axes.
Called by animate_basemodel_v2.py before saving; no new skeleton or runtime rig.
"""

import math


def build_combat_clips(api):
    begin, key, finish = (api[n] for n in ("begin", "key", "finish"))
    fill_rest, profile, ground = (api[n] for n in ("fill_rest", "profile", "ground"))
    bones, scene, D = api["BODY_BONES"], api["scene"], math.radians
    posed = {
        "root",
        "attach.tool.R",
        "hips",
        "torso",
        "head",
        "arm.L",
        "arm.R",
        "hand.L",
        "hand.R",
        "leg.L",
        "leg.R",
        "foot.L",
        "foot.R",
    }
    for name, frames in (
        ("combat_guard", 49),
        ("combat_strike", 25),
        ("combat_recoil", 13),
        ("combat_fall", 25),
        ("combat_fall_back", 25),
    ):
        begin(name)
        for f in range(1, frames + 1):
            p = (f - 1) / (frames - 1)
            breath = math.sin(p * math.tau) if name == "combat_guard" else 0
            # Impact is exactly .30 seconds into the one-second clip, matching
            # COMBAT_WINDUP_SECONDS. Keep recovery complete by .76 seconds.
            strike = (
                profile(
                    p,
                    [(0, 0), (0.16, -0.65), (0.30, 1), (0.40, 1.12), (0.70, 0), (1, 0)],
                )
                if name == "combat_strike"
                else 0
            )
            recoil = (
                profile(p, [(0, 0), (0.22, 1), (0.5, 0.65), (1, 0)])
                if name == "combat_recoil"
                else 0
            )
            dying = name in {"combat_fall", "combat_fall_back"}
            fall = (
                profile(p, [(0, 0), (0.18, 0.12), (0.65, 0.9), (1, 1)]) if dying else 0
            )
            backwards = name == "combat_fall_back"
            key(
                "root",
                f,
                loc=(0, 0.015 * recoil, 0),
                rot=(
                    D((86 if backwards else -86) * fall),
                    0,
                    D((-9 if backwards else 12) * fall),
                ),
            )
            key("hips", f, rot=(D(2), D(-7 + 16 * strike), 0))
            key(
                "torso",
                f,
                rot=(
                    D(5 + 11 * strike - 14 * recoil),
                    D(12 - 33 * strike),
                    D(breath - 3 * strike),
                ),
            )
            key("head", f, rot=(D(3 - 8 * recoil), D(-4 + 5 * strike), 0))
            # Short guarded cut. The blade stays in front throughout its arc.
            key(
                "arm.R",
                f,
                rot=(
                    D(-125 + 22 * strike + 10 * recoil + 125 * fall),
                    D(-8 * strike),
                    D(10 - 13 * strike),
                ),
            )
            key("hand.R", f, rot=(D(8 - 12 * strike - 14 * fall), 0, 0))
            key("attach.tool.R", f, rot=(0, D(180), 0))
            key("arm.L", f, rot=(D(-51 + 28 * fall + breath), 0, D(-15)))
            key("hand.L", f, rot=(D(12), 0, D(-8)))
            key("leg.L", f, rot=(D(-8 + 5 * fall), 0, D(3)))
            key("leg.R", f, rot=(D(7 - 10 * fall), 0, D(-3)))
            key("foot.L", f, rot=(D(8 - 5 * fall), 0, 0))
            key("foot.R", f, rot=(D(-7 + 10 * fall), 0, 0))
        fill_rest(posed, list(range(1, frames + 1)))
        act = finish(name, bones)
        scene.frame_start, scene.frame_end = 1, frames
        ground(act, list(range(1, frames + 1)))
        if not name.startswith("combat_fall"):
            api["check_loop"](name, frames)
