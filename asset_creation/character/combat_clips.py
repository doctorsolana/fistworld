"""Dedicated baseline sidearm poses, using the canonical rig's measured axes.
Called by animate_basemodel_v2.py before saving; no new skeleton or runtime rig.
"""
import math


def build_combat_clips(api):
    begin, key, finish = (api[n] for n in ("begin", "key", "finish"))
    fill_rest, profile, ground = (api[n] for n in ("fill_rest", "profile", "ground"))
    bones, scene, D = api["BODY_BONES"], api["scene"], math.radians
    posed = {"root", "hips", "torso", "head", "arm.L", "arm.R", "hand.L", "hand.R", "leg.L", "leg.R", "foot.L", "foot.R"}
    for name, frames in (("combat_guard", 49), ("combat_strike", 25), ("combat_recoil", 13), ("combat_fall", 25)):
        begin(name)
        for f in range(1, frames + 1):
            p = (f - 1) / (frames - 1)
            breath = math.sin(p * math.tau) if name == "combat_guard" else 0
            strike = profile(p, [(0, 0), (.16, -.35), (.30, 1), (.42, .8), (.76, 0), (1, 0)]) if name == "combat_strike" else 0
            recoil = profile(p, [(0, 0), (.22, 1), (.5, .65), (1, 0)]) if name == "combat_recoil" else 0
            fall = profile(p, [(0, 0), (.18, .12), (.65, .9), (1, 1)]) if name == "combat_fall" else 0
            key("root", f, loc=(0, .015 * recoil, 0), rot=(D(-86 * fall), 0, D(12 * fall)))
            key("hips", f, rot=(D(2), D(-7 + 9 * strike), 0))
            key("torso", f, rot=(D(5 + 8 * strike - 14 * recoil), D(7 - 13 * strike), D(breath)))
            key("head", f, rot=(D(3 - 8 * recoil), D(-4 + 5 * strike), 0))
            # Short guarded cut. The blade stays in front throughout its arc.
            key("arm.R", f, rot=(D(-63 - 26 * strike + 10 * recoil + 34 * fall), 0, D(10)))
            key("hand.R", f, rot=(D(20 - 24 * strike), D(170), 0))
            key("arm.L", f, rot=(D(-51 + 28 * fall + breath), 0, D(-15)))
            key("hand.L", f, rot=(D(24), 0, D(-8)))
            key("leg.L", f, rot=(D(-8 + 5 * fall), 0, D(3)))
            key("leg.R", f, rot=(D(7 - 10 * fall), 0, D(-3)))
            key("foot.L", f, rot=(D(8 - 5 * fall), 0, 0))
            key("foot.R", f, rot=(D(-7 + 10 * fall), 0, 0))
        fill_rest(posed, list(range(1, frames + 1)))
        act = finish(name, bones)
        scene.frame_start, scene.frame_end = 1, frames
        ground(act, list(range(1, frames + 1)))
        if name != "combat_fall":
            api["check_loop"](name, frames)
