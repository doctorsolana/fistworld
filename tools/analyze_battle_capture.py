#!/usr/bin/env python3
"""Measure connected battle captures without adding work to the live simulation.

Usage: python3 tools/analyze_battle_capture.py CAPTURE_DIRECTORY
Writes movement-analysis.json beside the inspected PNG / battle JSON sequence.
Waiting behind an engaged ally is reported separately from an unsupported stall.
These diagnostics supplement visual inspection; they are not an FPS benchmark.
"""
import argparse
import json
import math
from pathlib import Path


def distance(a, b):
    return math.hypot(a[0] - b[0], a[2] - b[2])


def analyze(directory, owner="battlelab"):
    tracks = {}
    overlap_samples = []
    combat_started = False
    for path in sorted(directory.glob("*.battle.json")):
        frame = json.loads(path.read_text())
        now = frame["world_seconds"]
        combat_started |= frame["contacts"] > 0
        people = [p for p in frame["people"] if p["health"] > 0]
        overlaps = 0
        for index, person in enumerate(people):
            for other in people[index + 1:]:
                overlaps += distance(person["position"], other["position"]) < 0.55
        overlap_samples.append(overlaps)
        for person in people:
            if person["owner"] != owner:
                continue
            enemies = [p for p in people if p["owner"] != person["owner"]]
            if not enemies:
                continue
            position = person["position"]
            nearest = min(distance(position, p["position"]) for p in enemies)
            support = nearest < 5.0 and any(
                p["owner"] == person["owner"] and p["target"] is not None
                and p["person"] != person["person"]
                and distance(position, p["position"]) < 3.0 for p in people
            )
            track = tracks.setdefault(person["person"], {
                "person": person["person"], "battalion": person["battalion"],
                "path_before_contact": 0.0, "initial_enemy_distance": nearest,
                "first_contact_at": None, "max_unsupported_stall_seconds": 0.0,
                "reversals": 0, "history": [], "last_direction": None,
            })
            history = track["history"]
            if history:
                previous = history[-1][1]
                step = distance(position, previous)
                if track["first_contact_at"] is None:
                    track["path_before_contact"] += step
                if step > 0.2:
                    direction = ((position[0] - previous[0]) / step,
                                 (position[2] - previous[2]) / step)
                    last = track["last_direction"]
                    if last and sum(a * b for a, b in zip(direction, last)) < -0.5:
                        track["reversals"] += 1
                    track["last_direction"] = direction
            if person["target"] is not None and track["first_contact_at"] is None:
                track["first_contact_at"] = frame["since_order"]
            # The fixture orders defenders to Hold while the attackers approach.
            # Waiting before first contact is intentional, not a movement stall.
            awaiting_attack = owner == "battlelab_enemy" and not combat_started
            history.append((now, position, person["target"] is not None, support or awaiting_attack))
            # A five-second window distinguishes actual congestion from a
            # normal brief pause while another soldier clears an approach.
            while len(history) > 1 and now - history[1][0] >= 5.0:
                history.pop(0)
            stalled = (now - history[0][0] >= 5.0 and nearest > 2.2
                       and not any(p[2] or p[3] for p in history)
                       and max(distance(position, p[1]) for p in history) < 0.4)
            if stalled:
                track.setdefault("stall_started", history[0][0])
                track["max_unsupported_stall_seconds"] = max(
                    track["max_unsupported_stall_seconds"], now - track["stall_started"])
            else:
                track.pop("stall_started", None)
    soldiers = []
    for track in tracks.values():
        for field in ["history", "last_direction", "stall_started"]:
            track.pop(field, None)
        soldiers.append(track)
    soldiers.sort(key=lambda p: (-p["max_unsupported_stall_seconds"], -p["reversals"]))
    result = {
        "owner": owner, "frames": len(overlap_samples), "soldiers_observed": len(soldiers),
        "soldiers_reaching_contact": sum(p["first_contact_at"] is not None for p in soldiers),
        "soldiers_with_unsupported_stalls": sum(p["max_unsupported_stall_seconds"] >= 5 for p in soldiers),
        "max_unsupported_stall_seconds": max((p["max_unsupported_stall_seconds"] for p in soldiers), default=0),
        "mean_close_overlap_pairs": sum(overlap_samples) / max(1, len(overlap_samples)),
        "peak_close_overlap_pairs": max(overlap_samples, default=0),
        "direction_reversals": sum(p["reversals"] for p in soldiers),
        "soldiers": soldiers,
    }
    (directory / ("movement-analysis.json" if owner == "battlelab" else "defender-movement-analysis.json")).write_text(json.dumps(result, indent=2) + "\n")
    return {key: value for key, value in result.items() if key != "soldiers"}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--owner", choices=["battlelab", "battlelab_enemy"], default="battlelab")
    args = parser.parse_args()
    print(json.dumps(analyze(args.directory, args.owner), indent=2))
