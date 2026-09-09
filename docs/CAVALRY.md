# Cavalry

Mounted sword cavalry is available in the connected battle lab. Start from the
repository root:

```sh
./run.sh cavalryworld
# Faster development build:
./run.sh cavalryworld --dev
```

The lab gives you two eight-rider battalions on the wings and eight infantry in
the centre, facing two sixteen-person infantry battalions. Combat mode is open
and the client stays under your control. Closing it stops the server started by
this command. An occupied local server port is reported without killing another
session. Logs go under ignored `logs/cavalryworld-*`.

## Controls

- Click a horse, its rider, or the **RIDERS** battalion card to select the battalion.
- Right-click terrain to move; drag the right mouse button for width and facing.
- Right-click an enemy to engage. Cavalry use the existing flexible individual
  melee approaches once they close with infantry.
- **H** cancels an objective; **X** arms attack-move; **R** retreats. The existing
  hold-line and defensive standing stances also apply to cavalry.
- **[ / ]** change formation width; **, / .** turn a selected battalion in place.
- Remove riders from their battalion in Army management to command them separately.
  They keep their horses and can join another cavalry battalion.

## Simulation and presentation

The rider is the authoritative selectable combat unit. Its horse is a paired
equipment entity with a session horse ID and rider `PersonId`. Both use the same
replicated ground position, facing and motion. `player/riding` owns pairing,
mount equipment and cleanup; ordinary orders, shared navigation fields and
`hero::step_units` own commanded movement. No second cavalry movement integrator
or per-rider A* exists.

Shared role-aware formation layout gives cavalry 3.2 m rank spacing and enough
file spacing for the enlarged horse. The same layout feeds the client preview
and server destinations. Horse terrain/prop clearance is 1.55 m. Mounted bodies
use wider crowd separation and melee contact distances than infantry.

The client keeps the person root at ground level for selection and attaches its
scene to the horse's animated `Anchor_Rider` transform. Seated lower-body clips
follow the horse gait phase; upper-body melee uses the existing authored strike
and reaction clips with animation masks. Face animation remains separate.
Horse rigs share a graph and assets. Mounted rigs have a separate bounded budget
from ambient animals; distant mounted horses use an authored static mesh proxy.

Horse, rider, melee, bow and arrow animation share a cosmetic presentation clock
that advances between authoritative clock packets. It follows time warp and
pause, resynchronizes after clock discontinuities, and extrapolates no more than
100 ms of real time beyond the latest update. It never changes `WorldTime` or
simulation outcomes. Fixed offline captures sample their authoritative fixture
clock exactly, preserving reproducible poses.

Provisioned mounts share the horse ID allocator with wildlife but do not consume
the 128 ambient-horse capacity or run grazing decisions. Their lifecycle follows
the soldier, including removal when the soldier dies or leaves military service.

## Current boundary

This is mounted melee cavalry, with faster travel and a larger footprint. It does
not yet implement a momentum/charge-damage model, trampling, lances, mounted
archery, separate horse health or a horse death animation. Rider casualties use
the ordinary soldier mortality pipeline and remove the supplied mount.

Stable recruitment, buying and supplying horses, taming, saddles and tack remain
future work. Public role changes cannot create free horses. The Army page keeps
cavalry equipment fixed so a role toggle cannot discard a lab-issued mount that
the player cannot yet replace.

## Verification

`capture/scenarios/battle-cavalry.ron` is the shared connected fixture. Supply it
to both server and client for the automatic version described in
[VISUAL-CAPTURE.md](VISUAL-CAPTURE.md); manual launch supplies it only to the server.
The automatic run checks normal battalion selection and attack acceptance,
paired visual readiness, at least 10 m of travel for every rider, mounted speed
above 3.5 m/s, mounted melee impacts, all three friendly battalions engaging and
casualties. Continuous PNGs have `.capture.json` and `.battle.json` evidence,
including rider/horse identity and clip phase. It also takes close riding/contact
views through the real Bevy renderer.

Run `cargo check --workspace --all-targets` and `cargo test --workspace` when
changing these contracts. Shared tests cover role serialization and layout;
server tests cover orders, equipment and pair cleanup; client tests cover picking
and Army controls. Captures are required in addition to those tests.

Validated on 2026-09-09: workspace/all-target check, paired development build and
**1,041 workspace tests passed** (18 ignored, no failures). In the final connected
run all 16 riders travelled 46–54 m and landed melee impacts, with peak speed
6 m/s, all three friendly battalions engaging and 31 total casualties. Inspected
the real close combat PNG and metadata at
`logs/captures/cavalry-connected-final/0015.png`.

The continuous presentation run passed all ten PNG/JSON probes: two full horse
rigs close, zero at zoom 850 with both proxies/riders retained, and the same two
pairs restored. Close/return poses and wide view were inspected under
`logs/captures/cavalry-visuals/`. The 1280×720 Army-page capture also passed and was
inspected under `logs/captures/ui-army-cavalry/`. These are functional and visual
checks, not a cavalry balance or comparative performance benchmark.

The existing connected archer scenario also passed after the mounted hit-volume
change: 77 arrows released, 14 archers visibly drawing at peak, 15 switching to
sidearms and 43 total casualties. Inspected its PNG/metadata under
`logs/captures/cavalry-archer-regression-final/`. Connected observation tests must
not change the calendar clock mid-run with a delayed debug time preset.

### Lag review, 2026-09-09

On the Apple M5 / 32 GB development machine, the release build ran the same
56-person cavalry scenario in a native borderless window with VSync and the
software cap disabled, continuous event-loop updates and metrics-only recording.
Compilation had finished before timing. Only the 3D render scale changed:

| 3D render scale | Scene dimensions | Mean FPS | p99 frame interval |
| --- | --- | --- | --- |
| 100% | 2940 × 1846 | 32.4 | 34.1 ms |
| 60% | 1764 × 1108 | 78.5 | 19.0 ms |

The instrumented server simulation phases averaged 1.5–1.7 ms combined, with
approximately 100% fixed-clock delivery. Rendering resolution is a substantial
cost in this case. These are application frame intervals, not GPU timestamps,
and do not establish performance for larger armies or other machines. The
windowed 1600 × 900 runs settled at 30 FPS; do not mix that different presentation
path into the borderless render-scale comparison.

The new cosmetic clock advanced on all 1,229 sampled native-resolution frames;
the replicated clock held its previous value on 217 of those frames. This fixes
packet-paced animation without claiming an algorithmic FPS gain. Timings, flags,
focus counters and initial render metadata are under the ignored
`logs/captures/cavalry-perf-final-{native,scaled}/` directories. Functional captures
are separate from these timing runs.

Follow-up verification passed: the workspace/all-target check, all 1,054 current
tests (18 ignored), connected cavalry and archer regressions, and all ten
continuous cavalry presentation probes. Inspected the PNGs and capture metadata
under `logs/captures/cavalry-lag-visual-profile/`,
`logs/captures/battle-lag-archer-regression/` and
`logs/captures/battle-lag-cavalry-visuals/`. The cavalry replay retained all 16
mounted attackers through their first melee impacts; the archer replay released
77 arrows and exercised the switch to sidearms.
