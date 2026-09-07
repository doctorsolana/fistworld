"""Work rhythms with aligned wrists and explicitly oriented tool grips.

The felling axe cuts horizontally into a trunk; the scythe follows the ground.
Wind-up, contact and recovery have distinct timing rather than a sine-wave wag.
"""

import math


def build_work_clips(api):
    begin, key, finish = (api[n] for n in ("begin", "key", "finish"))
    D, profile = math.radians, api["profile"]
    for name, period in (("build", 32), ("chop", 40), ("harvest", 48)):
        begin(name)
        frames = list(range(1, period + 2))
        for f in frames:
            p = (f - 1) / period
            stroke = profile(
                p,
                [
                    (0, 0),
                    (0.16, -0.10),
                    (0.32, 1),
                    (0.44, 1.08),
                    (0.68, 0.45),
                    (0.92, 0),
                    (1, 0),
                ],
            )
            if name == "build":
                yaw, pitch, roll = -7, 2 + 14 * stroke, 0
                right, left, tool_roll = (
                    -155 + 83 * stroke,
                    -22 - 10 * stroke,
                    180,
                )
                head = 10 + 8 * stroke
            elif name == "chop":
                yaw, pitch, roll = -48 + 86 * stroke, 3 + 7 * stroke, 5 - 11 * stroke
                right, left, tool_roll = (
                    -112 + 18 * stroke,
                    -92 - 16 * stroke,
                    -90,
                )
                head = 7
            else:
                # Smooth, low cutting stroke; more time allotted to sweeping
                # than chopping, followed by a visible lifted return.
                stroke = profile(
                    p, [(0, 0), (0.08, 0), (0.45, 1), (0.55, 1), (0.82, 0.25), (1, 0)]
                )
                yaw, pitch, roll = -45 + 83 * stroke, 20 - 3 * stroke, 4 - 8 * stroke
                right, left, tool_roll = (
                    -97 + 14 * stroke,
                    -75 + 14 * stroke,
                    -75,
                )
                head = -13
            key("root", f, loc=(0, 0, 0), rot=(0, 0, 0))
            key("hips", f, rot=(0, D(-0.28 * yaw), 0))
            key("torso", f, rot=(D(pitch), D(yaw), D(roll)))
            key("head", f, rot=(D(head), D(-0.45 * yaw), 0))
            key("arm.R", f, rot=(D(right), 0, D(7 if name != "build" else 0)))
            key("arm.L", f, rot=(D(left), 0, D(-13 if name != "build" else -6)))
            key(
                "hand.R",
                f,
                rot=(
                    D(8 - 16 * stroke if name == "build" else 5 - 7 * stroke),
                    0,
                    0,
                ),
            )
            key(
                "hand.L",
                f,
                rot=(
                    D(-8 if name == "build" else 5 - 7 * stroke),
                    0,
                    0,
                ),
            )
            # The tool can be gripped at a different roll without inverting a wrist.
            key("attach.tool.R", f, rot=(0, D(tool_roll), 0))
            # A stable, slightly staggered stance; the pelvis and torso drive
            # weight transfer, while the counter-rotated feet remain planted.
            for side, angle in (("L", -4), ("R", 4)):
                key("leg." + side, f, rot=(D(angle), 0, 0))
                key("foot." + side, f, rot=(D(-angle), 0, 0))
        api["fill_rest"](
            {
                "root",
                "attach.tool.R",
                "hips",
                "torso",
                "head",
                "arm.R",
                "arm.L",
                "hand.R",
                "hand.L",
                "leg.L",
                "leg.R",
                "foot.L",
                "foot.R",
            },
            frames,
        )
        act = finish(name, api["BODY_BONES"])
        api["ground"](act, frames)
        api["check_loop"](name, period + 1)
