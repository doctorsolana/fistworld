"""Read-only evidence for a self-building villager's interrupted construction.

Client and server ECS entity numbers differ, and unfinished sites lack OwnedBy.
Bind only an actor already Building at one unique replicated work stand, then
retain its PersonId and server assignment token to reject job changes.
This observer does not cover hired house-upgrade contractors.
"""
import json
import math
import re
import threading


BUILD_REACH = 4.0  # village::BUILD_REACH; this is an arrival check, not a stall tolerance.
FOOD_OBJECTIVES = {
    "QueuedForPersonalFood", "QueuedForPoorRelief", "CollectingFood", "Eating",
    "GoingHouseholdShopping", "QueuedForHouseholdFood", "ReturningWithHouseholdFood",
}
CONSTRUCTION_OBJECTIVES = {
    "FindingConstructionWood", "ChoppingTimber", "CarryingConstructionWood",
    "QueuedForConstructionWood", "CollectingConstructionWood", "ConstructingBuilding",
}


def distance(a, b):
    return math.hypot(a[0] - b[0], a[2] - b[2])


def enum_value(value):
    value = value or ""
    return value[5:-1] if value.startswith("Some(") and value.endswith(")") else value


class ConstructionMeals:
    def __init__(self):
        self.tracks = {}
        self.events = []

    def observe(self, row, sites):
        events = []
        for actor in row.get("actors", []):
            match = re.fullmatch(r"Some\(Building \{ settlement: [^,]+, site: ([^}]+) \}\)",
                                 actor.get("intent", ""))
            assignment = match.group(1) if match else None
            identity = actor["id"]
            objective = enum_value(actor.get("objective"))
            activity = enum_value(actor.get("activity"))
            # Clear a former commitment even when the replacement is idle,
            # another trade or an unobserved/unowned construction site.
            for other_key, other in self.tracks.items():
                if other_key != (identity, assignment) and other_key[0] == identity and other["stage"] != "resumed":
                    other["stage"] = "assignment_changed"
            key = (identity, assignment)
            track = self.tracks.get(key)
            if track is None:
                at_stand = [site for site in sites
                            if distance(actor["position"], site["stand"]) <= BUILD_REACH]
                if (not assignment or len(at_stand) != 1 or activity != "Building"
                        or objective != "ConstructingBuilding"):
                    continue
                track = {"person": identity, "assignment": assignment, "site": at_stand[0],
                         "stage": "construction", "last_work_position": actor["position"],
                         "food_service_seen": False, "food_displacement": 0.0}
                self.tracks[key] = track
                event = self.event("construction", row, actor, track)
                events.append(event)
            if track["stage"] in ("resumed", "assignment_changed"):
                continue
            site = next((site for site in sites if site["entity"] == track["site"]["entity"]), None)
            if site is None:
                continue
            track["site"] = site
            if objective in FOOD_OBJECTIVES:
                if track["stage"] == "construction":
                    track["stage"] = "food"
                    track["food_origin"] = track["last_work_position"]
                track["food_displacement"] = max(
                    track["food_displacement"], distance(actor["position"], track["food_origin"]))
                # Queue admission alone could be cancelled. Require actual
                # eating or a collected household basket before accepting it.
                track["food_service_seen"] |= objective in ("Eating", "ReturningWithHouseholdFood")
                if (track["stage"] == "food" and track["food_service_seen"]
                        and track["food_displacement"] >= 3.0):
                    track["stage"] = "returning"
                    events.append(self.event("food", row, actor, track))
            elif track["stage"] == "construction" and objective in CONSTRUCTION_OBJECTIVES:
                track["last_work_position"] = actor["position"]
            elif (track["stage"] == "returning" and activity == "Building"
                  and objective == "ConstructingBuilding" and site.get("raising")
                  and distance(actor["position"], site["stand"]) <= BUILD_REACH):
                track["stage"] = "resumed"
                events.append(self.event("resumed", row, actor, track))
        self.events.extend(events)
        return events

    @staticmethod
    def event(stage, row, actor, track):
        return {"event": stage, "elapsed": row["elapsed"], "actor": actor,
                "site": track["site"], "assignment": track["assignment"],
                "food_displacement": track["food_displacement"]}

    def report(self):
        return {"passed": any(track["stage"] == "resumed" for track in self.tracks.values()),
                "tracks": list(self.tracks.values()), "events": self.events,
                "scope": "Initial Building at a unique replicated work stand binds the actor/site. "
                         "Same self-building PersonId and retained construction assignment; "
                         "food service plus >=3m actual displacement, then Building within "
                         "the ordinary 4m reach of that original site's stand. No actor orders "
                         "or grants. Sampling may miss short phases; missing evidence is not a pass."}


class ContinuousSnapshots:
    """Retain fresh client states even while a screenshot command waits for readback."""
    def __init__(self, session, path):
        self.session = session
        self.path = path
        self.error = None
        self.sites = {}
        self.lock = threading.Lock()
        self.finished = threading.Event()
        self.thread = threading.Thread(target=self.collect, daemon=True)
        self.thread.start()

    def collect(self):
        previous = None
        try:
            with self.path.open("w") as output:
                while not self.finished.is_set():
                    try:
                        state = self.session.status()
                    except json.JSONDecodeError:
                        continue  # Retry a partially replaced live status file.
                    stamp = state.get("sampled_unix_ms")
                    if stamp is not None and stamp != previous:
                        previous = stamp
                        output.write(json.dumps(state, separators=(",", ":")) + "\n")
                        output.flush()
                        with self.lock:
                            for site in state.get("construction", {}).get("sites", []):
                                self.sites[site["entity"]] = site
                    self.finished.wait(.1)
        except BaseException as error:
            self.error = repr(error)

    def known_sites(self):
        if self.error:
            raise RuntimeError(f"Client evidence sampler failed: {self.error}")
        with self.lock:
            return list(self.sites.values())

    def close(self):
        self.finished.set()
        self.thread.join(timeout=5)
        if self.thread.is_alive() or self.error:
            raise RuntimeError(f"Client evidence sampler failed to finish: {self.error}")


def completed_captures(directory, name):
    """The Session reply follows screenshot completion; verify its actual artifacts too."""
    captures = []
    for png in sorted(directory.glob(f"{name}*.png")):
        metadata = png.with_suffix(".capture.json")
        state = png.with_suffix(".session.json")
        if not png.stat().st_size or not metadata.exists() or not state.exists():
            raise AssertionError(f"Incomplete capture evidence: {png}")
        capture = json.loads(metadata.read_text())
        session = json.loads(state.read_text())
        captures.append({"png": str(png), "metadata": str(metadata), "session": str(state),
                         "world": capture.get("world"), "sampled_unix_ms": session.get("sampled_unix_ms")})
    if not captures:
        raise AssertionError(f"No completed PNGs for {name}")
    return captures


def replay(directory):
    """Re-evaluate retained journals without controlling a running game."""
    def lines(path):
        with path.open() as source:
            for line in source:
                if not line.endswith("\n"):
                    break
                yield json.loads(line)

    baseline = json.loads((directory / 'meal-baseline.json').read_text())
    elapsed = baseline['server']['elapsed']
    timestamp = baseline['client']['sampled_unix_ms']
    states = iter(lines(directory / 'client-states.jsonl'))
    current = next(states, None)
    sites = {}
    observer = ConstructionMeals()
    for row in lines(directory / 'server-movement.jsonl'):
        if row['elapsed'] < elapsed:
            continue
        stamp = timestamp + (row['elapsed'] - elapsed) * 1000
        while current and current['sampled_unix_ms'] <= stamp:
            for site in current.get('construction', {}).get('sites', []):
                sites[site['entity']] = site
            current = next(states, None)
        observer.observe(row, list(sites.values()))
    report = observer.report()
    report['replay_scope'] = (
        'Server elapsed time is aligned to the paired client-ready baseline, '
        'within the existing sampling cadence. Only already recorded site geometry '
        'is admitted; ambiguous overlapping stands never bind. No new render evidence.')
    (directory / 'meal-replay.json').write_text(json.dumps(report, indent=2))
    return report


if __name__ == '__main__':
    import argparse
    from pathlib import Path
    parser = argparse.ArgumentParser(description='Recheck recorded construction/meal evidence only.')
    parser.add_argument('--replay', type=Path, required=True)
    report = replay(parser.parse_args().replay.resolve())
    print(json.dumps({'passed': report['passed'], 'tracks': len(report['tracks']),
                      'events': len(report['events'])}))
