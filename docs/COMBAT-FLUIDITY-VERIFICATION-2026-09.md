# Formation control and local combat flexibility — 2026-09-06

> Historical first pass. The selection/deployment controls remain, but the
> contact-face allocation and section bending described below were subsequently
> replaced after playtesting. See `COMBAT-DESIGN.md` for the current movement model
> and `COMBAT-LOOSE-MOVEMENT-VERIFICATION-2026-09.md` for its verification.

This pass makes a battalion the smallest controllable group while membership
exists. Clicking or box-selecting a member expands the selection, and the server
enforces the same boundary. Remove members through Army management for individual
control. Control groups retain battalion identities as membership changes.

Deployment uses shared, deterministic geometry. Battalions remember width and
spacing; the preview shows slots, footprints, facing and ranks. Right-drag sets
the frontage, an ordinary move retains it, brackets change width in place, and
comma/period rotate each selected battalion by 15 degrees. Normal multi-battalion
attacks distribute across the nearby enemy line; Ctrl/Cmd-right-click focuses the
clicked target. Combat fronts bend in small local sections, cover nearby empty
files, and share intervals on a target's faces. Hold line still forbids automatic
movement. See `COMBAT-DESIGN.md` for controls and implementation limits.

## Connected verification

All runs use the real server and Bevy client on `battle_lab`. Artifact paths below
are relative to `logs/playtests/fluid-formations/`. PNGs were inspected with their
capture metadata and per-person metrics. Captures exercise continuous movement
or combat, rather than substituting an offline scene for connected behaviour.

| Scenario / artifact directory | Result |
| --- | --- |
| `army-fluid-final` | 150 soldiers in three battalions completed all three deployments: 25 files × 2 ranks, an ordinary move retaining that shape, then a quarter-turn with a new width. All 150 arrived and aligned each time. Maximum position errors: 0.273, 0.256, 0.168 m. |
| `army-250-final` | Five 50-person battalions completed both deployments, including the quarter-turn. All 250 arrived and aligned. Maximum errors: 0.250 and 0.090 m. |
| `battle-3v3-final` | Three 50-person battalions versus three. Selection expansion verified; all three attacking battalions engaged; peak 60 contacts; 45.12 simulated seconds observed after contact; 161 captures. |
| `battle-3v1` | All three attacking battalions engaged one defender battalion; peak 28 contacts. Front and flank approaches visually inspected. |
| `battle-skirmish` | All three unassigned attackers joined an existing battalion clash in two waves; selection expansion verified; peak 26 contacts. Local reactions and retained supporting ranks visually inspected. |

The first fluid deployment run found a real spacing defect: a late arrival could
push a settled soldier 0.544 m away and leave them there indefinitely. Idle
Defensive formations now restore displaced slots, without activating pursuit or
overriding Hold line. The final runs retain the original 0.3 m position and
0.06 rad facing limits. A server regression also reproduces a 0.6 m displacement
and verifies recovery within 0.2 m.

The 3v1 and skirmish runs preceded that final idle-slot maintenance correction;
3v3 and both deployment scenarios were rerun with the final playtest binaries.
Keyboard reshape shortcuts are compiled and use the same server move boundary;
the connected driver exercises drag deployment, ordinary movement and rotation,
not synthetic presses of those shortcut keys.

## Code checks and limitations

- `cargo check --workspace --all-targets` passed after the idle-slot correction.
- `cargo build --workspace --profile playtest` passed for both runtime binaries.
- `cargo test --workspace` passed before the last idle-slot correction: 196 client,
  501 server and 206 shared tests, with 11 ignored.
- The final `cargo test --workspace --bin server` run passed 501 tests, including
  the added idle-slot regression; three were ignored. One unrelated asset test
  failed because `building_stone_quarry` was registered by concurrent asset work
  but absent from the current `client/assets/colliders.bin`. That asset database
  and registration were left with their owning work in progress.
- `git diff --check` passed. Build and test logs are retained under the artifact
  directory's `verification/` subdirectory.

These are behaviour and correctness checks, not an FPS benchmark. Local section
bending and interval allocation do not implement strategic tactical AI, rigid
formation wheeling or coordinated narrow-passage reservations. The fresh manual
3v3 session uses the scenario on the server only, leaving all client orders to
the player.
