#!/usr/bin/env python3
"""Rehearse live audio controls, semantic book cues and actual moving-cart output.

Launches only its own normal seeded server/client, uses first_session commands,
and records only that client PID. No simulation state or saved preferences are
modified. Numeric/capture success never implies that somebody listened to audio.
Run after matching playtest binaries and logs/tools/record_app_audio are built:

    python3 capture/sfx_review.py --out logs/sfx-review/run-01
"""

import argparse
import json
import math
import os
from pathlib import Path
import subprocess
import threading
import time

import first_session as harness


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_SEED = 4800834198907808058
MIN_CART_SECONDS = 6.0


def audio(state):
    return state.get("audio") or {}


def settings_match(state, **values):
    settings = audio(state).get("settings") or {}
    return all(settings.get(key) == value if isinstance(value, bool)
               else isinstance(settings.get(key), (int, float))
               and abs(settings[key] - value) <= 0.003 for key, value in values.items())


def music_matches(state, master, music):
    voices = [voice for voice in audio(state).get("music", []) if voice.get("sink")]
    return bool(voices) and all(
        voice.get("paused") is False and isinstance(voice.get("volume"), (float, int))
        and abs(voice["volume"] - master * music * (0.82 if voice["cue"] == "Opening" else 0.8)) <= 0.01
        for voice in voices)


def no_effect_voices(state):
    return not audio(state).get("ui") and not audio(state).get("cart_voices")


def started(state, cue):
    return (audio(state).get("stats") or {}).get("started", {}).get(cue, 0)


def audible_carts(state):
    return [voice for voice in audio(state).get("cart_voices", [])
            if voice.get("sink") and voice.get("estimated_gain", voice.get("gain", 0)) > 0.002]


def cart_evidence(samples, owner):
    """Require real source displacement and progressing, audible native sinks."""
    positions, playback = [], []
    audible_seconds = longest_audible_seconds = continuous_seconds = 0.0
    previous = None
    for sample in samples:
        state = sample["state"]
        voices = [voice for voice in audible_carts(state) if voice["owner"] == owner]
        sources = [source for source in audio(state).get("cart_sources", []) if source["entity"] == owner]
        if sources:
            positions.append(sources[0]["position"])
        if voices:
            playback.append(voices[0].get("playback_position"))
        if previous is not None and previous[1] and voices:
            # Evidence is published about once a second; a stale/missing interval
            # cannot silently count toward the required continuous observation.
            gap = sample["monotonic"] - previous[0]
            if 0 < gap <= 2.0:
                audible_seconds += gap
                continuous_seconds += gap
                longest_audible_seconds = max(longest_audible_seconds, continuous_seconds)
            else:
                continuous_seconds = 0.0
        else:
            continuous_seconds = 0.0
        previous = (sample["monotonic"], bool(voices))
    travel = sum(harness.distance(left, right) for left, right in zip(positions, positions[1:]))
    progressing = any(left is not None and right is not None and abs(right - left) > 0.1
                      for left, right in zip(playback, playback[1:]))
    return {"owner": owner, "sample_count": len(samples), "audible_seconds": audible_seconds,
            "longest_continuous_audible_seconds": longest_audible_seconds,
            "observed_travel_metres": travel, "decoder_progress_observed": progressing,
            "passed": longest_audible_seconds >= MIN_CART_SECONDS and travel > 0.5 and progressing}


class ReviewSession(harness.Session):
    def __init__(self, directory, deadline, processes):
        super().__init__(directory)
        self.deadline = deadline
        self.processes = processes

    def remaining(self, maximum):
        left = self.deadline - time.monotonic() - 20.0  # Reserve bounded cleanup.
        if left <= 0:
            raise TimeoutError("Ten-minute audio rehearsal budget exhausted")
        for name, process in self.processes.items():
            if name in ("server", "client") and process.poll() is not None:
                raise RuntimeError(f"Owned {name} exited with {process.returncode}")
        return min(maximum, left)

    def wait(self, predicate, description, timeout=60):
        deadline = time.monotonic() + self.remaining(timeout)
        while time.monotonic() < deadline:
            self.remaining(timeout)
            state = self.status()
            if state and predicate(state):
                return state
            time.sleep(0.1)
        raise TimeoutError(f"{description}; inspect {self.directory / 'status.json'}")

    def command(self, action, *, timeout=35, **parameters):
        with (self.directory / "audio-actions.jsonl").open("a") as journal:
            journal.write(json.dumps({"wall_time": time.time(), "monotonic": time.monotonic(),
                                      "action": action, **parameters}) + "\n")
        return super().command(action, timeout=self.remaining(timeout), **parameters)


class Sampler:
    def __init__(self, session, path):
        self.session, self.path = session, path
        self.samples, self.phase = [], "startup"
        self.done = threading.Event()
        self.thread = threading.Thread(target=self.run, name="audio-evidence", daemon=True)

    def run(self):
        stamp = None
        while not self.done.wait(0.1):
            try:
                current = (self.session.directory / "status.json").stat().st_mtime_ns
                if current == stamp:
                    continue
                state = self.session.status()
                stamp = current
                sample = {"monotonic": time.monotonic(), "wall_time": time.time(),
                          "phase": self.phase, "state": {key: state.get(key) for key in
                          ("audio", "camera", "hero", "clock", "ui_blocking")}}
                self.samples.append(sample)
                with self.path.open("a") as journal:
                    journal.write(json.dumps(sample) + "\n")
            except (FileNotFoundError, json.JSONDecodeError):
                continue

    def stop(self):
        self.done.set()
        self.thread.join(timeout=1)


def stop_owned(process):
    if process is not None and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=3)


def snapshot(session, out, name):
    state = session.status()
    (out / "snapshots").mkdir(exist_ok=True)
    (out / "snapshots" / f"{name}.json").write_text(json.dumps(state, indent=2) + "\n")
    return state


def capture(session, report, name, *, frames=1, interval_ms=500):
    if frames == 1:
        session.command("capture", name=name, timeout=80)
        names = [name]
    else:
        session.command("record", name=name, frames=frames, interval_ms=interval_ms, timeout=80)
        names = [f"{name}-{index:04}" for index in range(frames)]
    for shot in names:
        for suffix in (".png", ".capture.json", ".session.json"):
            assert (session.directory / (shot + suffix)).is_file(), f"Missing capture artifact: {shot}{suffix}"
    report["captures"].extend(names)


def open_audio(session):
    if harness.enabled(session.status(), "audio-master-slider"):
        return
    if not harness.enabled(session.status(), "pause-AUDIO"):
        session.command("key", key="Escape")
        session.wait(lambda state: harness.enabled(state, "pause-AUDIO"), "pause menu")
    session.command("button", name="pause-AUDIO")
    session.wait(lambda state: harness.enabled(state, "audio-master-slider"), "Audio page")


def close_pause(session):
    # Escape dismisses the Audio subpanel before the containing pause menu.
    harness.close_book(session)


def slider(session, control, value):
    session.command("slider", name=f"audio-{control}-slider", value=value)
    return session.wait(lambda state: settings_match(state, **{f"{control}_volume": value}),
                        f"live {control} slider at {value}")


def record_start(session, out, processes, label, recorder):
    output = out / f"{label}.wav"
    stop, ready = out / f"{label}.stop", out / f"{label}.ready"
    with (out / f"{label}.recorder.log").open("w") as log:
        process = subprocess.Popen([str(recorder), str(processes["client"].pid), str(output),
                                    str(stop), str(ready), "120"], cwd=ROOT,
                                   stdout=log, stderr=subprocess.STDOUT)
    processes["recorder"] = process
    deadline = time.monotonic() + session.remaining(20)
    while not ready.exists():
        if process.poll() is not None:
            raise RuntimeError(f"App-audio recorder failed; inspect {label}.recorder.log")
        if time.monotonic() >= deadline:
            raise TimeoutError(f"App-audio recorder did not become ready: {label}")
        time.sleep(0.1)
    return {"output": output, "stop": stop, "ready": ready, "process": process}


def record_stop(session, recording, report):
    recording["stop"].touch()
    recording["process"].wait(timeout=session.remaining(12))
    path = recording["output"].with_suffix(".wav.capture-audio.json")
    assert recording["process"].returncode == 0, f"Recorder failed: {path}"
    data = json.loads(path.read_text())
    assert data["application_filter_only"] and data["microphone"] is False
    assert data["sample_rate"] == 48000 and data["channels"] == 2
    assert data["frames"] > 0 and data["nonzero_frames"] > 0, f"No actual app signal: {path}"
    assert all(math.isfinite(value) and 0 <= value < 1 for value in data["peak_linear_by_channel"]), \
        f"Captured mix clips or has invalid peaks: {path}"
    report["recordings"].append({"path": str(recording["output"]), "metadata": data})
    return data


def find_cart(session, timeout=90):
    before = {}
    chosen = {}

    def moved(state):
        for source in audio(state).get("cart_sources", []):
            old = before.get(source["entity"])
            before[source["entity"]] = source["position"]
            if old is not None and source.get("speed", 0) > 0.4 \
                    and harness.distance(old, source["position"]) > 0.05:
                chosen.update(source)
                return True
        return False

    session.wait(moved, "observed moving physical cart in the largest town", timeout)
    return chosen


def focus_cart(session, source):
    x, _, z = source["position"]
    session.command("view", x=x, z=z, zoom=24)
    return session.wait(lambda state: any(voice["owner"] == source["entity"]
                        for voice in audible_carts(state))
                        and abs((state.get("camera") or {}).get("zoom", 0) - 24) < 0.5,
                        "near cart sink, nonzero gain and settled close zoom", timeout=40)


def run(args):
    began = time.monotonic()
    out, recorder = args.out.resolve(), args.recorder.resolve()
    if not out.is_relative_to(ROOT / "logs") or (out.exists() and any(out.iterdir())):
        raise ValueError("Use a fresh output directory under this repository's logs/")
    if not 60 <= args.timeout <= 600:
        raise ValueError("Total timeout must be between 60 and 600 seconds")
    for binary in (ROOT / "target/playtest/server", ROOT / "target/playtest/client", recorder):
        if not binary.is_file() or not os.access(binary, os.X_OK):
            raise ValueError(f"Build the required executable first: {binary}")
    if subprocess.run(["lsof", "-nP", "-iUDP:5000"], capture_output=True).returncode == 0:
        raise RuntimeError("UDP 5000 is occupied; existing processes are left alone")
    out.mkdir(parents=True, exist_ok=True)
    processes = {}
    sampler = recording = None
    preferences = ROOT / "client_data/audio.ron"
    old_preferences = preferences.read_bytes() if preferences.exists() else None
    report = {"seed": args.seed, "resolution": args.resolution, "passed": False,
              "listening_approved": False, "visuals_personally_inspected": False,
              "captures": [], "recordings": [], "cart_attempts": [],
              "scope": "Owned normal server/client, production UI inputs and camera-only cart observation",
              "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()}
    try:
        env = harness.environment(ROOT) | {"FISTWORLD_WORLD_SEED": str(args.seed), "FISTWORLD_DEV": "0",
                                           "FISTFORCE_SERVER_PERF": "1"}
        with (out / "server.log").open("w") as log:
            processes["server"] = subprocess.Popen([ROOT / "target/playtest/server"], cwd=ROOT,
                                                    env=env, stdout=log, stderr=subprocess.STDOUT)
        session = ReviewSession(out / "client", began + args.timeout, processes)
        harness.wait_server(processes["server"], out / "server.log",
                            lambda text: "Server UDP socket bound" in text,
                            "normal seeded server ready", timeout=session.remaining(240))
        env = harness.environment(ROOT) | {
            "FISTFORCE_NO_SETTINGS_FILE": "1", "FISTFORCE_AUTOCONNECT": "AudioReview",
            "FISTFORCE_DISPLAY_MODE": "windowed", "FISTFORCE_FULLSCREEN": "0",
            "FISTFORCE_RESOLUTION": args.resolution, "FISTFORCE_RENDER_SCALE": "1",
            "FISTFORCE_FRAME_CAP": "60", "FISTWORLD_SESSION_CAPTURE_DIR": str(session.directory),
        }
        with (out / "client.log").open("w") as log:
            processes["client"] = subprocess.Popen([ROOT / "target/playtest/client"], cwd=ROOT,
                                                    env=env, stdout=log, stderr=subprocess.STDOUT)
        (out / "processes.json").write_text(json.dumps({name: process.pid for name, process in processes.items()}, indent=2))
        sampler = Sampler(session, out / "audio-timeline.jsonl")
        sampler.thread.start()
        state = session.wait(lambda state: state.get("playing") and state.get("creator"),
                             "ordinary new-player creator", timeout=150)
        assert not state.get("god") and state.get("hero") is None
        session.command("button", name="creator-BEGIN JOURNEY")
        state = session.wait(lambda state: state.get("hero") and not state.get("cinematic")
                             and len(state.get("towns", [])) >= 10, "hero and world directory", timeout=150)
        session.wait(lambda state: any(voice.get("sink") and not voice.get("paused")
                     for voice in audio(state).get("music", [])), "active native music decoder", timeout=45)
        town = max(state["towns"], key=lambda town: (town["residents"], -town["id"]))
        report["town"] = town
        sampler.phase = "audio_controls"
        recording = record_start(session, out, processes, "ui-controls", recorder)
        open_audio(session)
        for control, value in (("master", 0.6), ("music", 0.3), ("effects", 0.5)):
            slider(session, control, value)
        session.wait(lambda state: music_matches(state, 0.6, 0.3), "live music sink reflects both sliders")
        snapshot(session, out, "01-live-levels")
        capture(session, report, "01-audio-levels")
        session.command("button", name="pause-music-off")
        session.wait(lambda state: settings_match(state, music_enabled=False, music_volume=0.3)
                     and all(not voice.get("sink") or voice.get("paused")
                             or voice.get("volume", 0) <= 0.001
                             for voice in audio(state).get("music", [])), "music mute preserves slider")
        slider(session, "master", 0)
        session.wait(no_effect_voices, "master zero releases all effect voices")
        snapshot(session, out, "02-master-zero")
        slider(session, "master", 1)
        slider(session, "effects", 0)
        session.wait(no_effect_voices, "effects zero releases all effect voices")
        snapshot(session, out, "03-effects-zero")
        slider(session, "effects", 1)
        assert settings_match(session.status(), master_volume=1, effects_volume=1,
                              music_volume=0.3, music_enabled=False, effects_enabled=True)
        close_pause(session)
        before = session.status()
        session.command("key", key="N")
        opened = session.wait(lambda state: started(state, "BookOpen") > started(before, "BookOpen")
                              and state.get("ui_blocking"), "semantic book open")
        assert started(opened, "BookOpen") == started(before, "BookOpen") + 1
        assert started(opened, "UiClick") == started(before, "UiClick"), "Book open doubled its generic click"
        for tab in ("Places", "People"):
            before_page = session.status()
            session.command("button", name=f"encyclopedia-tab-{tab}")
            turned = session.wait(lambda state: started(state, "PageTurn") > started(before_page, "PageTurn"),
                                  f"semantic page turn to {tab}")
            assert started(turned, "PageTurn") == started(before_page, "PageTurn") + 1
            assert started(turned, "UiClick") == started(before_page, "UiClick"), "Page turn doubled generic click"
        capture(session, report, "02-book")
        before_close = session.status()
        session.command("key", key="Escape")
        closed = session.wait(lambda state: started(state, "BookClose") > started(before_close, "BookClose")
                              and not state.get("ui_blocking"), "semantic book close")
        assert started(closed, "BookClose") == started(before_close, "BookClose") + 1
        snapshot(session, out, "04-book-semantics")
        record_stop(session, recording, report)
        recording = None
        sampler.phase = "town_streaming"
        x, _, z = town["position"]
        session.command("view", x=x, z=z, zoom=105)
        session.wait(lambda state: any(market["id"] == town["id"] for market in state.get("markets", [])),
                     "largest town replicated", timeout=90)
        capture(session, report, "03-largest-town")
        source = None
        for attempt in range(1, 4):
            source = find_cart(session, timeout=60)
            focus_cart(session, source)
            phase = f"near-cart-{attempt}"
            capture(session, report, f"near-cart-ready-{attempt}")
            sampler.phase = f"starting-{phase}"
            recording = record_start(session, out, processes, phase, recorder)
            start = time.monotonic()
            sampler.phase = phase
            # Real continuous Bevy frames, with ordinary motion unchanged. The
            # telemetry sampler keeps publishing while this command is active.
            capture(session, report, phase, frames=14, interval_ms=500)
            session.wait(lambda _: time.monotonic() - start >= 12,
                         "twelve seconds of actual close-cart observation", timeout=15)
            samples = [sample for sample in sampler.samples if sample["phase"] == phase]
            evidence = cart_evidence(samples, source["entity"])
            evidence["wall_observation_seconds"] = time.monotonic() - start
            data = record_stop(session, recording, report)
            recording = None
            evidence["recorded_seconds"] = data["duration_seconds"]
            evidence["passed"] = evidence["passed"] and data["duration_seconds"] >= MIN_CART_SECONDS
            report["cart_attempts"].append(evidence)
            if evidence["passed"]:
                break
            # A real cart can finish a job during the clip. Retain that evidence
            # and select another actual mover, with at most three bounded tries.
            session.command("view", x=x, z=z, zoom=105)
        assert report["cart_attempts"][-1]["passed"], "No cart supplied six seconds of moving/audible evidence"
        snapshot(session, out, "05-near-cart")
        from audio_distance_review import review_distance
        review_distance(session, out, report, sampler, processes, recorder)
        sampler.phase = "far_zoom"
        session.command("view", x=source["position"][0], z=source["position"][2], zoom=1000)
        session.wait(lambda state: not audio(state).get("cart_voices")
                     and (state.get("camera") or {}).get("zoom", 0) > 950,
                     "far zoom releases physical cart voices", timeout=30)
        snapshot(session, out, "06-far-zoom")
        session.wait(lambda state: audio(state).get("filtered_sources") == 0,
                     "far zoom releases filtered voice assets as well as sinks")
        capture(session, report, "04-far-zoom")
        sampler.phase = "return_near"
        session.command("view", x=x, z=z, zoom=105)
        source = find_cart(session, timeout=60)
        focus_cart(session, source)
        snapshot(session, out, "07-return-near")
        sampler.phase = "effects_live_mute"
        recording = record_start(session, out, processes, "cart-live-mute", recorder)
        open_audio(session)
        session.command("button", name="pause-effects-off")
        session.wait(lambda state: settings_match(state, effects_enabled=False, effects_volume=1)
                     and no_effect_voices(state), "Effects toggle clears live UI/cart voices", timeout=20)
        snapshot(session, out, "08-effects-muted")
        capture(session, report, "05-effects-muted")
        record_stop(session, recording, report)
        recording = None
        report["final_audio"] = audio(session.status())
        session.wait(lambda state: audio(state).get("filtered_sources") == 0,
                     "mute releases filtered voice assets")
        report["book_keys"] = ["N", "Escape"]
        report["page_tabs"] = ["Places", "People"]
        assert max((len(audio(sample["state"]).get("cart_voices", [])) for sample in sampler.samples), default=0) <= 4
        assert max((len(audio(sample["state"]).get("ui", [])) for sample in sampler.samples), default=0) <= 4
        report["passed"] = True
    except Exception as error:
        report["error"] = str(error)
        raise
    finally:
        if recording is not None and recording["process"].poll() is None:
            recording["stop"].touch()
            try:
                recording["process"].wait(timeout=3)
            except subprocess.TimeoutExpired:
                pass
        if sampler is not None:
            sampler.stop()
        for name in ("recorder", "client", "server"):
            stop_owned(processes.get(name))
        report["elapsed_seconds"] = time.monotonic() - began
        report["saved_audio_preferences_unchanged"] = (
            preferences.read_bytes() if preferences.exists() else None) == old_preferences
        if not report["saved_audio_preferences_unchanged"]:
            report["passed"] = False
            report["error"] = "Capture modified saved audio preferences"
        (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    if not report["passed"]:
        raise AssertionError(report.get("error", "Audio review did not pass"))
    print(f"Numeric/session review complete: {out / 'report.json'}. Inspect PNG/metadata and listen to WAVs separately.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--seed", type=int, default=DEFAULT_SEED)
    parser.add_argument("--resolution", default="1600x1000")
    parser.add_argument("--timeout", type=float, default=600)
    parser.add_argument("--recorder", type=Path, default=ROOT / "logs/tools/record_app_audio")
    run(parser.parse_args())


if __name__ == "__main__":
    main()
