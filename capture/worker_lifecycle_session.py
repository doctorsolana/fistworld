#!/usr/bin/env python3
"""Observe genuine fishing/herding work, cargo, delivery and resumed work at 1x/25x.

The connected worker-lifecycle fixture stages initial conditions only. This driver
uses ordinary launcher/God speed controls and read-only camera/session capture.
It never places workers, grants production or patches their routine state.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import shutil
import subprocess
import time

from first_session import distance, environment, stop, wait_server
from multiplayer_session import claim_udp_port, start_client
from startup_session import join, menu, replace

ROOT = Path(__file__).resolve().parents[1]
ROLES = {"FishermansHut": ("fish", "Fishing", 1.1),
         "LivestockFarm": ("meat", "Farming", 1.8)}


class Journal:
    """Read every complete sample, including samples written during captures."""
    def __init__(self, path):
        self.path = path
        self.offset = 0
        self.latest = None

    def read(self):
        if not self.path.exists():
            return []
        rows = []
        with self.path.open() as source:
            source.seek(self.offset)
            while line := source.readline():
                if not line.endswith("\n"):
                    break
                self.offset = source.tell()
                row = json.loads(line)
                self.latest = row
                rows.append(row)
        return rows


class Lifecycle:
    def __init__(self, site, cycles):
        self.site = site
        self.good, self.activity, self.reach = ROLES[site["kind"]]
        self.required_cycles = cycles
        self.worker = None
        self.stage = "work"
        self.completed = 0
        self.last = None
        self.entry_cargo = 0
        self.work_samples = 0
        self.return_seen = False
        self.events = []

    def observe(self, row):
        stock = next(s for s in row["sites"] if s["id"] == self.site["id"])
        candidates = [a for a in row["actors"] if a["workplace"] == self.site["id"]]
        if not candidates:
            return []
        actor = next((a for a in candidates if a["id"] == self.worker), candidates[0])
        if actor["id"] != self.worker:
            if self.worker is not None and self.stage != "work":
                raise AssertionError(f"Worker changed during a physical cycle: {self.site['kind']}")
            self.worker = actor["id"]
            self.last = None
        if actor.get("simulation") != "canonical":
            raise AssertionError(f"Worker evidence is not from canonical physical simulation: {actor}")
        working = actor["activity"] == self.activity
        if working and distance(actor["position"], self.site["work_point"]) > self.reach:
            raise AssertionError(f"Work animation away from its real work point: {actor}")
        cargo = actor[self.good]
        event = None
        if self.stage == "work" and working:
            self.stage = "cargo"
            self.entry_cargo = cargo
            self.work_samples = 1
            event = "working"
        elif self.stage == "cargo":
            self.work_samples += int(working)
            if cargo > self.entry_cargo and self.work_samples >= 2:
                self.stage = "deposit"
                self.return_seen = False
                event = "carrying"
        elif self.stage == "deposit" and self.last:
            prior_actor, prior_stock = self.last
            dropped = prior_actor[self.good] - cargo
            received = stock[self.good] - prior_stock[self.good]
            if dropped > 0 and received > 0:
                assert distance(actor["position"], self.site["entrance"]) <= 2.0, actor
                assert dropped == received, (actor, stock, self.last)
                if self.good == "meat":
                    assert stock["wool"] - prior_stock["wool"] == prior_actor["wool"] - actor["wool"] > 0
                self.stage = "resume"
                event = "deposited"
            elif (not working and cargo > 0 and not self.return_seen
                  and distance(actor["position"], prior_actor["position"]) > .05):
                self.return_seen = True
                event = "returning"
        elif self.stage == "resume" and working:
            self.completed += 1
            self.stage = "cargo"
            self.entry_cargo = cargo
            self.work_samples = 1
            event = "resumed"
        self.last = (actor, stock)
        if event:
            evidence = {"event": event, "elapsed": row["elapsed"], "day": row["day"],
                        "world_seconds": row["world_seconds"], "warp": row["warp"],
                        "actor": actor, "stock": stock, "completed_cycles": self.completed,
                        "observed_work_samples": self.work_samples}
            self.events.append(evidence)
            return [evidence]
        return []

    def passed(self):
        return self.completed >= self.required_cycles


def set_warp(session, factor):
    # Native focus can move while a long camera capture is running. The normal
    # God shortcut intentionally ignores an unfocused game window.
    if not session.status().get("window_focused"):
        session.command("focus_window")
        session.wait(lambda s: s.get("window_focused"), "native game window focused")
    if session.status().get("hud_mode") != "God":
        session.command("key", key="G")
    session.wait(lambda s: s.get("hud_mode") == "God", "real God speed controls")
    session.command("button", name=f"god-warp-{factor}")
    session.wait(lambda s: s.get("time_warp") == factor, f"authoritative {factor}x")
    session.command("key", key="G")
    session.wait(lambda s: s.get("hud_mode") != "God", "ordinary HUD restored")


def frame_site(session, site, event):
    outward = math.atan2(site["work_point"][0] - site["position"][0],
                         site["work_point"][2] - site["position"][2])
    if event == "working":
        centre, zoom, yaw = site["work_point"], 24, outward
    else:
        centre = [(a + b) * .5 for a, b in zip(site["entrance"], site["work_point"])]
        zoom, yaw = 36, outward + math.pi * .5
    session.command("view", x=centre[0], z=centre[2], zoom=zoom, yaw=yaw)
    session.wait(lambda s: distance(s["camera"]["focus"], centre) < 1.0
                 and abs(s["camera"]["zoom"] - zoom) < .1,
                 "worker camera settled")


def ready_for_capture(site, event, actor):
    if actor is None:
        return False
    _, activity, reach = ROLES[site["kind"]]
    if event == "working":
        return (actor["activity"] == activity
                and distance(actor["position"], site["work_point"]) <= reach)
    objective = ("ReturningCatch" if site["kind"] == "FishermansHut"
                 else "ReturningLivestockProducts")
    return (actor["activity"] != activity and actor.get("objective") == objective
            and bool((actor.get("load") or {}).get("good")))


def capture_event(session, site, factor, event, captured, report):
    # One continuous work and one continuous carriage view per role/speed.
    if event["event"] not in ("working", "returning"):
        return
    name = f"{factor}x-{site['kind'].lower()}-{event['event']}"
    if name in captured:
        return
    frame_site(session, site, event["event"])
    # A queued event can become stale while another recording is captured,
    # especially at 25x. Preserve that distinction instead of mislabelling it.
    state = session.status()
    actor = next((p for p in state.get("households", {}).get("people", [])
                  if p["id"] == event["actor"]["id"]), None)
    if not ready_for_capture(site, event["event"], actor):
        return
    captured.add(name)
    reply = session.command("record", name=name, frames=12, interval_ms=120, timeout=90)
    report["captures"].append({"name": name, "trigger": event, "ready_client_actor": actor, "reply": reply})


def binary_identity(path):
    with path.open("rb") as source:
        checksum = hashlib.file_digest(source, "sha256").hexdigest()
    return {"path": str(path.resolve()), "sha256": checksum}


def run_phase(session, journal, fixture, factor, timeout, cycles, processes, report, out):
    set_warp(session, factor)
    journal.read()  # Earlier physics belongs to the prior phase, never this claim.
    started = time.monotonic()
    tracks = [Lifecycle(site, cycles) for site in fixture["sites"]]
    captured = set()
    phase = {"warp": factor, "required_cycles": cycles, "passed": False,
             "start_sample": journal.latest, "workers": []}
    report["phases"].append(phase)
    try:
        while time.monotonic() - started < timeout:
            if any(process.poll() is not None for process in processes):
                raise RuntimeError("An owned game process exited")
            pending = []
            for row in journal.read():
                if row["warp"] != factor:
                    continue
                if factor > 1 and row.get("sample_stage") != "after_work_before_movement":
                    raise AssertionError("Accelerated acceptance requires the current per-tick pre-movement trace")
                for track in tracks:
                    for event in track.observe(row):
                        pending.append((track.site, event))
                        print(f"{factor}x {track.site['kind']}: {event['event']} Person#{event['actor']['id']}", flush=True)
            for site, event in pending:
                capture_event(session, site, factor, event, captured, report)
            if all(track.passed() for track in tracks):
                phase["passed"] = True
                return
            time.sleep(.1)
        raise TimeoutError(f"{factor}x worker lifecycle incomplete: " + str([(t.site['kind'], t.stage, t.completed) for t in tracks]))
    finally:
        phase["elapsed_seconds"] = time.monotonic() - started
        phase["workers"] = [{"site": t.site, "person_id": t.worker, "completed_cycles": t.completed,
                             "stage": t.stage, "events": t.events} for t in tracks]
        phase["end_sample"] = journal.latest
        (out / "report.json").write_text(json.dumps(report, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--resolution", default="1280x800")
    parser.add_argument("--normal-timeout", type=float, default=1000,
                        help="real seconds; paired livestock production needs at least eight minutes at 1x")
    parser.add_argument("--fast-timeout", type=float, default=240)
    parser.add_argument("--warps", type=int, choices=[1, 25], nargs="+", default=[1, 25],
                        help="speeds to validate, in order; --warps 25 retains a separate accelerated-only result")
    parser.add_argument("--server", type=Path, default=ROOT / "target/playtest/server")
    parser.add_argument("--client", type=Path, default=ROOT / "target/playtest/client")
    args = parser.parse_args()
    out = args.out.resolve()
    if not out.is_relative_to(ROOT / "logs"):
        raise ValueError("Keep acceptance artifacts under logs/")
    out.mkdir(parents=True, exist_ok=False)
    port = claim_udp_port(args.port)
    if args.client.resolve() != ROOT / "target/playtest/client":
        raise ValueError("The maintained startup helper uses target/playtest/client")
    report = {"passed": False, "scenario": "worker-lifecycle", "port": port,
              "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
              "binaries": {label: binary_identity(path)
                           for label, path in (("server", args.server), ("client", args.client))},
              "requested_warps": args.warps, "phases": [], "captures": [],
              "scope": "Initial fixture only. Normal job hiring, physical production/cargo/deposit and return to work at the explicitly requested speeds. No accelerated warmup counted as 1x. An accelerated-only run makes no normal-speed claim. Not an FPS or long-run economy benchmark."}
    server = client = wake_guard = session = None
    try:
        env = environment(ROOT) | {"CITYSIM_MAP_ID": "village_lab", "FISTWORLD_VILLAGE_LAB_RUNTIME": "1",
            "FISTWORLD_LAB_SCENARIO": "worker-lifecycle", "FISTWORLD_WORKER_TRACE_DIR": str(out),
            "FISTWORLD_LAB_WARP": "1", "FISTWORLD_NATURAL_IMMIGRATION": "0", "FISTWORLD_DEV": "1",
            "FISTWORLD_SERVER_PORT": str(port), "FISTFORCE_SERVER_PERF": "1"}
        with (out / "server.log").open("w") as log:
            server = subprocess.Popen([args.server.resolve()], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        if caffeinate := shutil.which("caffeinate"):
            wake_guard = subprocess.Popen([caffeinate, "-ds", "-w", str(server.pid)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        wait_server(server, out / "server.log", lambda text: f"Server UDP socket bound to 0.0.0.0:{port}" in text,
                    "worker fixture server listening", timeout=180)
        wait_server(server, out / "server.log", lambda text: "Worker lifecycle fixture ready" in text,
                    "validated initial workplaces", timeout=180)
        fixture = json.loads((out / "fixture.json").read_text())
        report["fixture"] = fixture
        client, session = start_client(ROOT, out / "client", args.resolution)
        (out / "processes.json").write_text(json.dumps({"server": server.pid, "client": client.pid}))
        menu(session)
        replace(session, "startup-server-field", f"127.0.0.1:{port}")
        join(session, "WorkerLifecycleQA")
        session.wait(lambda s: s.get("playing") and s.get("creator") and s.get("god_capability"), "ordinary creator and server developer grant")
        session.command("button", name="creator-BEGIN JOURNEY")
        session.wait(lambda s: s.get("hero") and not s.get("cinematic") and not s.get("creator"), "observer opening complete", timeout=180)
        centre = fixture["hall"]
        session.command("view", x=centre[0], z=centre[2], zoom=90)
        session.wait(lambda s: len([p for p in s.get("households", {}).get("people", []) if p["id"] in fixture["people"]]) >= 2,
                     "replicated fixture workers", timeout=90)
        session.command("capture", name="00-initial-workplaces")
        journal = Journal(out / "workers.jsonl")
        initial_rows = journal.read()
        report["hiring"] = [{"site": site["id"], "first_observed_employment": next(
            ({"elapsed": row["elapsed"], "day": row["day"], "warp": row["warp"], "actor": actor}
             for row in initial_rows for actor in row["actors"] if actor["workplace"] == site["id"]), None)}
            for site in fixture["sites"]]
        report["client_ready_baseline"] = {"client": session.status(), "server": journal.latest}
        for factor in args.warps:
            timeout, cycles = (args.normal_timeout, 1) if factor == 1 else (args.fast_timeout, 2)
            run_phase(session, journal, fixture, factor, timeout, cycles, [server, client], report, out)
        report["passed"] = True
        print("WORKER LIFECYCLE ACCEPTANCE PASSED; inspect the continuous PNGs and capture JSONs", flush=True)
    except BaseException as error:
        report["error"] = repr(error)
        raise
    finally:
        if session:
            report["last_client_state"] = session.status()
        (out / "report.json").write_text(json.dumps(report, indent=2))
        stop(client)
        stop(server)
        stop(wake_guard)


if __name__ == "__main__":
    main()
