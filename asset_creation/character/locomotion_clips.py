"""Run and breaststroke clips for the existing rigid, knee-less skeleton.

Run contact is derived from the feet; swim is authored relative to the water
surface (root origin), with the face above it and the trunk below it.
"""

import math


def build_locomotion_clips(api):
    begin, key, finish = (api[n] for n in ("begin", "key", "finish"))
    scene = api["scene"]
    D = math.radians
    # A complete left/right cycle at 24 fps. The source closes at frame 17.
    frames = list(range(1, 18))
    begin("run")
    for f in frames:
        t = (f - 1) / 16 * math.tau
        swing = math.cos(t)
        key("root", f, loc=(0, 0, 0), rot=(0, 0, 0))
        key("hips", f, rot=(0, D(4 * swing), 0))
        key("torso", f, rot=(D(11), D(-5 * swing), D(2 * swing)))
        key("head", f, rot=(D(-9), D(3 * swing), 0))
        for side, sign in (("L", 1), ("R", -1)):
            leg = 38 * swing * sign
            key("leg." + side, f, rot=(D(leg), 0, 0))
            key("foot." + side, f, rot=(D(-leg + 3 * math.sin(t) * sign), 0, 0))
            key("arm." + side, f, rot=(D(-8 - 36 * swing * sign), 0, D(-5 * sign)))
            key("hand." + side, f, rot=(D(12), 0, 0))
    api["fill_rest"](
        {
            "root",
            "hips",
            "torso",
            "head",
            "leg.L",
            "leg.R",
            "foot.L",
            "foot.R",
            "arm.L",
            "arm.R",
            "hand.L",
            "hand.R",
        },
        frames,
    )
    act = finish("run", api["BODY_BONES"])
    api["ground"](act, frames)
    # A brief flight phase at the passing pose, with unchanged planted contacts.
    curve = api["root_z"](act)
    for kp in curve.keyframe_points:
        t = (kp.co.x - 1) / 16 * math.tau
        lift = 0.018 * math.sin(t) ** 4
        kp.co_ui.y += lift
        kp.handle_left.y = kp.handle_right.y = kp.co_ui.y
    curve.update()
    api["check_loop"]("run", 17)

    # Breaststroke gives this rigid rig a readable reach / pull / glide. Root
    # tilts around its negative-X local axis; -76° lays the body face-down.
    for name, moving in (("swim", True), ("swim_idle", False)):
        begin(name)
        period = 40 if moving else 48
        frames = list(range(1, period + 2))
        for f in frames:
            p = (f - 1) / period
            t = p * math.tau
            pull = api["profile"](p, [(0, 0), (0.22, 0), (0.50, 1), (0.70, 1), (1, 0)])
            key(
                "root",
                f,
                loc=(0, 0, -0.27 + 0.018 * pull if moving else -0.60),
                rot=(D(-76 if moving else -20), 0, 0),
            )
            key("hips", f, rot=(0, 0, 0))
            key("torso", f, rot=(D(-3), 0, 0))
            key("head", f, rot=(D(-62 - 6 * pull if moving else -8), 0, 0))
            for side, sign in (("L", 1), ("R", -1)):
                key(
                    "arm." + side,
                    f,
                    rot=(
                        D(-164 + 86 * pull if moving else -62 + 10 * math.sin(t)),
                        0,
                        D(
                            sign
                            * (-(8 + 58 * math.sin(math.pi * pull)) if moving else -30)
                        ),
                    ),
                )
                key("hand." + side, f, rot=(D(8 + 12 * pull), 0, D(10 * sign * pull)))
                key(
                    "leg." + side,
                    f,
                    rot=(
                        D(5 * math.sin(t) if moving else 10 * math.sin(t) * sign),
                        0,
                        D(sign * (3 + 12 * pull if moving else 5)),
                    ),
                )
                key("foot." + side, f, rot=(D(-12), 0, 0))
        api["fill_rest"](
            {
                "root",
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
            },
            frames,
        )
        finish(name, api["BODY_BONES"])
        api["check_loop"](name, period + 1)
    # An intentional rest, distinct from either fatal fall. Descend slowly,
    # settle onto the back, then breathe with hands resting over the trunk.
    for name, period in (("lie_down", 36), ("lie_idle", 72)):
        begin(name)
        frames = list(range(1, period + 2))
        for f in frames:
            p = (f - 1) / period
            settle = (
                api["profile"](p, [(0, 0), (0.2, 0.06), (0.7, 0.85), (1, 1)])
                if name == "lie_down"
                else 1
            )
            breath = math.sin(p * math.tau) if name == "lie_idle" else 0
            key("root", f, loc=(0, 0, 0), rot=(D(89 * settle), 0, 0))
            key("hips", f, rot=(0, 0, 0))
            key("torso", f, rot=(D(-3 * settle + 0.3 * breath), 0, 0))
            key("head", f, rot=(D(-6 * settle), D(8 * settle), 0))
            for side, sign in (("L", 1), ("R", -1)):
                key("arm." + side, f, rot=(D(-24 * settle), 0, D(-6 * sign * settle)))
                key("hand." + side, f, rot=(D(10 * settle), 0, 0))
                key("leg." + side, f, rot=(0, 0, D(3 * sign * settle)))
                key("foot." + side, f, rot=(D(-8 * settle), 0, 0))
        api["fill_rest"](
            {
                "root",
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
            },
            frames,
        )
        act = finish(name, api["BODY_BONES"])
        api["ground"](act, frames)
        if name == "lie_idle":
            api["check_loop"](name, period + 1)
    scene.frame_set(1)
