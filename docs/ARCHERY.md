# Archers

Implemented 2026-09-07 across shared contracts, authoritative combat, existing
bow assets and retained Army controls. Visual verification results below are
recorded separately from implementation claims.

## Player controls

- Army management: choose a battalion, then **Archers** or **Infantry**.
- Right-click an enemy: advance to useful bow range, settle and shoot that enemy
  group. The focus modifier and whole-battalion selection work as for infantry.
- **V** toggles fire at will / hold fire for selected archer battalions. The Army
  page has the same buttons. Hold fire stops pending draws; arrows already flying
  continue. A new attack command resumes fire.
- Normal movement and retreat interrupt shooting. Attack-move stops to engage
  enemies within effective range, then resumes when the area clears.
- **H** cancels the current objective. Standing **Hold line** prevents automatic
  movement, including bombardment evasion; nearby enemies can still be fought.
- Quivers hold **24 arrows**. **Rearm quivers** and equipment changes require
  stationary troops with no enemies within **100 m**. Transfers and role toggles
  preserve remaining ammunition. Detached troops retain their equipment; re-forming an all-archer battalion also preserves bows and ammunition.
- Spread the line with the existing right-drag frontage controls when allies
  screen the rear ranks' shots.

`./run.sh archerworld` builds and opens a manual 120-vs-120 battle: two infantry
battalions and one archer battalion against three enemy battalions. Enemies
counterattack after shooting begins. Closing this client stops its own server.

## Combat contract

Maximum acquisition range is 65 m; attack approaches settle at about 42–45 m.
The line keeps its occupied ground and facing while its target remains in range.
Its anchor accounts for both file and rank offsets, including incomplete rows;
repeatedly treating an uneven formation's centroid as its front centre causes drift.
Individual archers can aim independently. Approaches reuse formation routing and
`hero::step_units`; no second movement integrator or per-soldier A* was added.

A draw lasts 1 s; the authored clip's release, recoil and recovery occupy a 2 s
clip, with a roughly 2.6–2.9 s firing cycle. Death, movement, a current hit reaction,
loss of a valid target, or hold fire cancels an unfinished draw. Cooldowns survive
new orders. The bow and humanoid sample the same absolute world-clock timestamp.
`BOW_RELEASE_LOCAL` is measured from `Humanoid/bow_shoot` at 1.0 s and the bow's
`ArrowRelease` node: (-0.417315, 1.162306, 0.060152) m. Re-measure it after changing
these assets. The arrow mesh starts at the nock and points +Z.

Arrows launch at 38 m/s under 9.81 m/s² gravity. The low ballistic solution leads
bounded target velocity; a small seeded angular spread is applied only at release.
Arrows never home. Each body impact deals 32 damage through the normal health,
reaction and mortality pipeline. Firing lanes test terrain, baked convex building
and prop shapes, and friendly bodies. A friend crossing after release can be hit.
Swept arrowhead segments prevent tunnelling; dead bodies cannot receive more damage.
Misses stop on scenery/ground and disappear after 3 s; free flight is bounded at
8 s. Body hits use the existing reaction rather than leaving floating arrows.

Enemies within 5 m cause a switch to sword combat. Archers only draw their bow
again after enemies remain outside 9 m for 2 s. Empty quivers also select the
sidearm, after the final shot's recovery. Existing infantry contact, spacing and
hold-line rules own the resulting melee.

## Ownership and cost

- `shared/components/archery.rs`: equipment, ammunition, fire policy, animation and
  projectile wire data; pure ballistic math. The current protocol ID lives in
  `shared/src/protocol/config.rs`; client and server must be rebuilt together.
- `server/player/archery`: equipment validation, weapon transitions, staggered
  target decisions, shared baked collision shapes and swept flight/damage.
- `client/hero/archery.rs`: the existing cached body/bow animation graphs.
  `hero/arrows.rs` reuses the authored arrow scene and samples its launch locally.
- Army roster/model/view/binding and the tactical command keys own player controls.

Ranged targeting uses a coarser index alongside the melee body grid and staggered
0.25–0.33 s decisions. Convex shapes are built at server initialization and shared;
building broad phase updates on geometry edits. Live arrows reuse body-grid scratch
storage, and collision substeps and lifetimes are bounded. Network traffic consists
of shot timestamps, ammunition changes, one analytic launch and one optional stop;
no arrow position is streamed every frame.

Rearming currently represents preparing fresh equipment away from battle. Supply
costs, armour/shield penetration, mounted archery, skill progression and individual
fire-policy controls for detached troops remain future work. Detached archers can
still receive move/hold/attack orders; the fire-policy buttons target battalions.

Mounted melee targets use one combined horse/rider health pool. Arrow impact and
friendly-fire tests share a rotated compound volume covering the horse and the
elevated rider, including a wider projectile broad phase. Infantry retain their
original capsule. See [CAVALRY.md](CAVALRY.md) for mounted-unit limitations.

## Verification

- `battle-archers.ron`: 16 archers, 32 infantry counterattack; requires real arrows
  and a transition to archer melee, alongside normal contact/casualty assertions.
- `battle-mixed-archers.ron`: 120 vs 120, with archers behind two infantry units.
- `battle-archer-animation.ron`: close view of four live archers drawing and
  releasing, then switching to swords against eight infantry.
- `army-archers.ron`: ordinary movement/frontage and the tactical HUD.
- `army-archer-management.ron`: retained Army page, bulk remove/refill, transfers,
  and hold-line/defensive bombardment behavior with archer battalions.
- Focused tests cover ballistic interception, swept bodies, convex walls, cancelled
  draws, friendly screens/crossings, ammunition/ownership, close-range hysteresis,
  detached approaches, attack-move pause/resume and incomplete-rank stability.

Verified 2026-09-07 with paired playtest binaries and the real connected renderer:

- Full workspace tests: **942 passed, 11 ignored, 0 failed**. Workspace/all-targets
  check and paired playtest build passed.
- 16 archers vs 32 infantry: **84 arrows**, peak 10 in flight, 13 archers entered
  melee. The uneven rear rank stayed on its deployed anchor.
- Four-archer close view: **21 arrows**, all four switched to melee. Personally
  inspected draw/release poses in `battle-archer-animation-final/0024.png` and
  its `.capture.json` (real connected scene, zoom 10).
- Mixed 120 vs 120: **286 arrows**, peak 35 in flight, all three friendly
  battalions engaged and 29 archers entered melee. A second metrics-only run
  fired 331 arrows, showed all 40 archers drawing, and maintained approximately
  100% world-clock delivery. Inspected the active fight in
  `battle-mixed-archers-review/0050.png` and its `.capture.json`.
- Army management: bulk removal/refill and transfers passed. Hold-line troops
  travelled 0 m under bombardment; defensive troops moved at least 13.31 m.
  Inspected the composed Army page and its capture metadata.

These artifacts are under `logs/playtests/loose-combat/`. They are live simulation
runs, not image baselines or clean comparative performance benchmarks. Capture
summaries also require a visible character with the authored shooting clip active;
server shots alone are insufficient visual evidence. Detailed clip state remains
in `.battle.json` for diagnosing future presentation regressions.
