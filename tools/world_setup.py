#!/usr/bin/env python3
"""Choose the host's world recipe before starting a new FistWorld server.

Prompts go to stderr; stdout contains only the selected config path for run.sh.
The server validates the RON again and sends the actual recipe to every client.
"""
from datetime import datetime
import math
from pathlib import Path
import sys
import uuid

ROOT = Path(__file__).resolve().parents[1]


def ask(label, default, parse=str, valid=lambda value: True):
    while True:
        print(f"{label} [{default}]: ", end="", file=sys.stderr, flush=True)
        line = sys.stdin.readline()
        if not line:
            raise SystemExit("World setup cancelled: no input.")
        value = line.strip() or str(default)
        try:
            result = parse(value)
            if valid(result):
                return result
        except ValueError:
            pass
        print("Please enter a value in the shown range.", file=sys.stderr)


def main():
    print("\nFistWorld — New world\n"
          "  1  Small frontier: 20% area, 4 Halls, 6 settlers each, 3 arrivals per day across the world\n"
          "  2  Established world: full size, developed settlements, natural immigration\n"
          "  3  Custom world\n"
          "This config belongs to the host. Joining players use the host's world.\n",
          file=sys.stderr)
    choice = ask("World", 1, int, lambda v: 1 <= v <= 3)
    if choice != 3:
        path = ROOT / "config/worlds" / ("small-frontier.ron" if choice == 1 else "mature.ron")
        print(path)
        return
    area = ask("World area, percent of full size (0.025–100)", 20, float,
               lambda v: math.isfinite(v) and .025 <= v <= 100)
    opening = ask("Settlement start: 1 = bare Moot Halls, 2 = developed towns", 1, int,
                  lambda v: v in (1, 2))
    count = ask("Number of settlements (1–32)", 4, int, lambda v: 1 <= v <= 32)
    founders = ask("Settlers per Hall (1–128)", 6, int, lambda v: 1 <= v <= 128) if opening == 1 else 6
    arrivals = ask("Boat arrivals per day across the whole world (0–20)", 3, float,
                   lambda v: math.isfinite(v) and 0 <= v <= 20)
    minimum = count * founders if opening == 1 else 1
    cap = ask(f"World NPC population limit ({minimum}–50000)", max(1000, minimum), int,
              lambda v: minimum <= v <= 50000)
    seed = ask("Seed: random, or a whole number", "random", str,
               lambda v: v == "random" or (v.isascii() and v.isdigit() and int(v) <= 2**64-1))
    size = 8192 * math.sqrt(area / 100)
    print(f"\nWorld: {size / 1000:.2f} × {size / 1000:.2f} km. "
          "Settlements need enough suitable dry land; the server checks this before play.", file=sys.stderr)
    directory = ROOT / "logs/world-setup"
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / f"custom-{datetime.now():%Y%m%d-%H%M%S}-{uuid.uuid4().hex[:6]}.ron"
    seed_ron = "None" if seed == "random" else f"Some({int(seed)})"
    stock = "Some((bread: 20, wheat: 6, wood: 12))" if opening == 1 else "None"
    path.write_text(f"""// Host-selected recipe. Copy this file to reuse these settings.
(
    world_area_scale: {area / 100!r},
    settlement_count: {count},
    founders_per_settlement: {founders},
    opening: {"Frontier" if opening == 1 else "Mature"},
    immigrants_per_day: Some({arrivals!r}),
    world_npc_cap: {cap},
    seed: {seed_ron},
    hall_stock: {stock},
    hall_treasury_pennies: None,
)
""")
    print(f"Saved settings: {path}\n", file=sys.stderr)
    print(path)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        raise SystemExit("\nWorld setup cancelled.")
