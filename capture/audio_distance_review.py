"""Camera-only distance rehearsal used by sfx_review; no simulation overrides."""
import json
import time

from sfx_review import (audio, audible_carts, capture, find_cart, focus_cart,
                        record_start, record_stop)


def review_distance(session, out, report, sampler, processes, recorder):
    attempts = report["distance_attempts"] = []
    for attempt in range(1, 4):
        source = find_cart(session, timeout=30)
        state = focus_cart(session, source)
        owner = source["entity"]
        original = next(v for v in audible_carts(state) if v["owner"] == owner)
        entity = original["voice_entity"]
        evidence = {"owner": owner, "voice_entity": entity, "passed": False, "phases": []}
        attempts.append(evidence)
        recording = None
        try:
            recording = record_start(session, out, processes, f"distance-{attempt}", recorder)
            for label, zoom, seconds in (("close", 12, 2), ("town", 100, 8), ("return", 12, 2)):
                sampler.phase = f"distance-{attempt}-{label}"
                samples = []
                phase = {"name": label, "zoom": zoom, "samples": samples}
                evidence["phases"].append(phase)
                until, start = time.monotonic() + 25, None
                while time.monotonic() < until:
                    state = session.status()
                    voice = next((v for v in audible_carts(state) if v["owner"] == owner), None)
                    current = next((v for v in audio(state).get("cart_sources", [])
                                    if v["entity"] == owner), None)
                    if voice is None or current is None or voice["voice_entity"] != entity:
                        raise AssertionError("Observed cart stopped/left; continuous voice trial ended")
                    samples.append({**voice, "zoom": state["camera"]["zoom"],
                                    "source_position": current["position"]})
                    if abs(state["camera"]["zoom"] - zoom) < 0.5:
                        if start is None:
                            start = voice["playback_position"]
                        if voice["playback_position"] - start >= seconds:
                            break
                    x, _, z = current["position"]
                    session.command("view", x=x, z=z, zoom=zoom)
                    previous = voice["playback_position"]
                    session.wait(lambda s: not any(v["voice_entity"] == entity for v in audible_carts(s))
                                 or any(v["voice_entity"] == entity and v["playback_position"] >= previous + 0.3
                                        for v in audible_carts(s)), "same decoder advances", timeout=4)
                else:
                    raise AssertionError("Distance phase did not settle and advance")
                phase["final"] = samples[-1]
                capture(session, report, f"distance-{attempt}-{label}")
            close, town, returned = [p["final"] for p in evidence["phases"]]
            assert max(s["cutoff_hz"] for p in evidence["phases"] for s in p["samples"]) <= 2400
            assert town["cutoff_hz"] < close["cutoff_hz"] * 0.7
            assert town["gain"] < close["gain"] * 0.65
            assert returned["cutoff_hz"] > town["cutoff_hz"] * 1.4
            assert returned["playback_position"] > close["playback_position"] + 8
            assert 0 < audio(session.status())["filtered_pcm_bytes"] <= 2 * 1024 * 1024
            evidence["passed"] = True
        except (AssertionError, TimeoutError) as error:
            evidence["error"] = str(error)
        finally:
            if recording is not None:
                record_stop(session, recording, report)
            (out / "distance-evidence.json").write_text(json.dumps(attempts, indent=2) + "\n")
        if evidence["passed"]:
            return
    raise AssertionError("No uninterrupted live distance-filter trial; inspect distance-evidence.json")
