#!/usr/bin/env python3
"""Write deterministic real-renderer horse/rider clip scenarios into a review directory.
Usage: python3 capture/horse_animations.py logs/captures/horse-animation-scenarios
Then pass each generated RON to target/playtest/capture --scenario PATH.
"""

import sys
from pathlib import Path

out = Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)
reviews = [
    (clip, None, seconds)
    for clip, seconds in [
        ("idle", 3),
        ("alert", 2),
        ("graze", 4),
        ("walk", 2),
        ("trot", 2.1),
        ("canter", 2.1),
        ("gallop", 1.8),
    ]
] + [
    ("idle", "ride_idle", 3),
    ("trot", "ride_trot", 2.1),
    ("gallop", "ride_gallop", 1.8),
    ("idle", "mount", 1.5),
    ("idle", "dismount", 1.5),
]
for horse, rider, seconds in reviews:
    name = rider or ("horse_" + horse)
    env = {
        "FISTFORCE_CAPTURE_ASSET": "game_assets/environment/animals/Horse.glb#Scene0",
        "FISTFORCE_CAPTURE_ASSET_CLIP": "horse_" + horse,
        "FISTFORCE_CAPTURE_CLOUDS": "clear",
    }
    if rider:
        env["FISTFORCE_CAPTURE_RIDER_CLIP"] = rider
    environment = ",".join(f'"{k}":"{v}"' for k, v in env.items())
    # Side view exposes hoof contact and grazing; front-left shows both rider hands.
    if rider:
        pose = "focus:(-18.8,0.,-18.8),yaw:-2.35619449,pitch:Some(0.10),eye:10.8"
    else:
        pose = "focus:(-10.,0.,-15.),yaw:1.57079633,pitch:Some(0.),eye:10.035553"
    # First focus also stages the asset, so hold one explicit origin shot before
    # entering the moving clip sequence. The first in-sequence shot uses the view above.
    shots = ['(name:"origin",focus:(-15.,0.,-15.),yaw:-2.55,zoom:8.,time_of_day:.43)']
    shots += [
        f'(name:"{name}-{i:03}",{pose},zoom:8.,time_of_day:.43)'
        for i in range(round(seconds * 30) + 1)
    ]
    text = f'''(version:1,name:"{name}",map:"village_lab",output_dir:"logs/captures/horse-animations/{name}",
resolution:(1400,1000),fixed_delta_seconds:0.033333333,target:scene,show_window:false,
warmup_frames:120,readiness:(minimum_frames:60,maximum_frames:1200,minimum_loaded_chunks:25,stable_loaded_chunk_frames:20),
continuous:true,probe_every:3,environment:{{{environment}}},shots:[{",".join(shots)}])\n'''
    (out / (name + ".ron")).write_text(text)
