#!/usr/bin/env python3
"""Write repeatable RON scenarios for the real UI; no alternate mock layouts."""
import argparse
import json
from pathlib import Path

PAGES = {
    "menu": {"FRONTEND": "menu"},
    "name": {"FRONTEND": "name"},
    "hero": {"HERO_CREATOR": "1"},
    "army": {"ENCYCLOPEDIA": "army"},
    "people": {"ENCYCLOPEDIA": "people"},
    "places": {"ENCYCLOPEDIA": "places", "SETTLEMENT": "village", "SELECT_PLACE": "Brackwater"},
    "retinue": {"ENCYCLOPEDIA": "retinue"},
    "companies": {"ENCYCLOPEDIA": "companies", "SETTLEMENT": "village"},
    "company": {"ENCYCLOPEDIA": "business", "SETTLEMENT": "village", "SELECT": "building"},
    "company-lower": {"ENCYCLOPEDIA": "business", "SETTLEMENT": "village", "SELECT": "building", "BUSINESS_SCROLL": "900"},
    "ledger": {"ENCYCLOPEDIA": "ledger", "SETTLEMENT": "village", "HISTORY": "company"},
    "founding": {"ENCYCLOPEDIA": "founding", "SELECT": "1"},
    "property": {"PROPERTY": "1", "SETTLEMENT": "village", "HUD": "god", "SELECT": "1"},
    "market": {"TRADE": "1", "ENCYCLOPEDIA": "places", "SETTLEMENT": "village", "SELECT": "1"},
    "settlement": {"SETTLEMENT": "village", "SELECT": "hall", "HUD": "player"},
    "pause": {"PAUSE": "main"},
    "graphics": {"PAUSE": "graphics"},
    "controls": {"PAUSE": "controls"},
    "map": {"WORLD_MAP": "1", "SELECT": "1", "SETTLEMENT": "village"},
    "developer": {"DEBUG_MENU": "god"},
    "combat": {"HUD": "player", "WARBAR": "1"},
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--only", help="Comma-separated page names")
    args = parser.parse_args()
    names = args.only.split(",") if args.only else list(PAGES)
    args.directory.mkdir(parents=True, exist_ok=True)
    for name in names:
        env = {"FISTFORCE_CAPTURE_HERO": "default", "FISTFORCE_CAPTURE_HERO_OFFSET": "0,0"}
        env.update({"FISTFORCE_CAPTURE_" + k: v for k, v in PAGES[name].items()})
        if name == "combat":
            env["FISTFORCE_COMBAT_MODE"] = "1"
        environment = "\n".join(f"        {json.dumps(k)}: {json.dumps(v)}," for k, v in env.items())
        minimum_chunks = 0 if "FRONTEND" in PAGES[name] else 25
        scenario = f'''(
    version: 1, name: "ui-{name}", map: "battle_lab",
    output_dir: "logs/captures/ui-theme/{name}",
    resolution: (1600, 900), fixed_delta_seconds: 0.016666667,
    target: window, show_window: false, warmup_frames: 180,
    readiness: (minimum_frames: 90, maximum_frames: 1200,
        minimum_loaded_chunks: {minimum_chunks}, stable_loaded_chunk_frames: 20),
    environment: {{
{environment}
    }},
    shots: [(name: "{name}", focus: (0.0, 0.0, 0.0), yaw: -0.45,
        zoom: 160.0, time_of_day: 0.5,
        assertions: [loaded_chunks_at_least(count: {minimum_chunks})])],
)
'''
        (args.directory / f"{name}.ron").write_text(scenario)


if __name__ == "__main__":
    main()
