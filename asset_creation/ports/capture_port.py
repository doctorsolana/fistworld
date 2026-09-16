"""Use the existing Bevy capture harness with an isolated asset root under logs.

Requires the ignored PortReview.glb assembled by review_port.py. Does not install
the draft into client/assets, register a building, or change simulation code.
"""
import math
import os
from pathlib import Path
import subprocess

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[1]
staging=ROOT/'logs/reviews/port-draft/assets'
staging.mkdir(parents=True,exist_ok=True)
for item in (ROOT/'client/assets').iterdir():
    target=staging/item.name
    if not target.exists() and not target.is_symlink():target.symlink_to(item,target_is_directory=item.is_dir())
target=staging/'port-draft'
if not target.exists():target.symlink_to(HERE/'renders',target_is_directory=True)
assert (target/'PortReview.glb').is_file(), 'Run review_port.py first'

views=[('harbour',(31,38,28),(-.4,7.8,1.8)),
       ('shore-office',(19,13,14),(-1.5,-.6,2.5)),
       ('dock-head',(-22,36,15),(0,17.6,1.6))]
shots=['(name: "anchor", focus: (-15.0, 0.0, -15.0), yaw: 2.55, zoom: 68.0, time_of_day: 0.43)']
for name,eye,aim in views:
    dx,dy,dz=[eye[i]-aim[i] for i in range(3)]
    yaw=math.atan2(dx,-dy);pitch=math.atan2(dz,math.hypot(dx,dy))
    shots.append(f'''(name: "{name}", focus: ({eye[0]-15:.6f}, 0.0, {-eye[1]-15:.6f}),
        yaw: {yaw:.8f}, zoom: 50.0, pitch: Some({pitch:.8f}), eye: {eye[2]+14.885553:.6f}, time_of_day: 0.43,
        assertions: [loaded_chunks_at_least(count: 25), prop_roots_at_most(count: 0), grass_batches_at_most(count: 0)])''')
recipe='''// Standalone harbour art review; all asset mounts live in ignored logs/.
(version: 1, name: "port-draft", map: "village_lab",
 output_dir: "logs/captures/port-draft", resolution: (1600, 1200),
 fixed_delta_seconds: 0.016666667, target: scene, show_window: false, warmup_frames: 240,
 readiness: (minimum_frames: 90, maximum_frames: 1200, minimum_loaded_chunks: 25, stable_loaded_chunk_frames: 20),
 environment: {"FISTFORCE_CAPTURE_ASSET": "port-draft/PortReview.glb#Scene0", "FISTFORCE_CAPTURE_CLOUDS": "clear"},
 shots: [\n'''+',\n'.join(shots)+'\n])\n'
scenario=HERE/'port-review.ron';scenario.write_text(recipe)
with (ROOT/'logs/reviews/port-draft/bevy.log').open('w') as log:
    subprocess.run([str(ROOT/'target/playtest/capture'),'--scenario',str(scenario)],
        cwd=ROOT,env={**os.environ,'BEVY_ASSET_ROOT':str(staging)},stdout=log,stderr=subprocess.STDOUT,check=True)
