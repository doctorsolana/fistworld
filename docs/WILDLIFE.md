# Wildlife and horses

Wild horses are part of the ordinary world. The server places small herds on dry,
gently sloping meadow grass and retains each horse's identity and ground position
for the lifetime of that running world. Looking away does not roll a new horse.
World restart still resets animals along with settlements and accounts; disk saves
are not implemented.

## Simulation ownership

- `shared/src/components/horse.rs` owns horse identity, compact activity timestamps,
  the seven animation clip contracts, and navigation dimensions.
- `server/src/world/wildlife/population.rs` builds seed-ordered habitat candidates
  once. It rejects water, steep slopes, other biomes, blocked ground and overlapping
  horses. Initial vegetation checks use the same deterministic blocking props and
  baked radii as navigation, including chunks nobody has streamed yet.
- `server/src/world/wildlife/behavior.rs` chooses at most 32 nearby animals across
  all observers. Observation is reconsidered twice per real second with a 25 m
  exit margin. Active horses alternate grazing, idle/alert poses and short walks
  around their home patch. They certify ground segments and avoid other horses.
- Distant horses have no movement, route search or behavioral updates. Their
  small records remain on the server and still use ordinary region interest
  filtering. This is deliberately a stationary offscreen wildlife policy, not
  a claim that migration, feeding or breeding is being simulated strategically.
- Inactive horses do not anchor collider streaming. Activation requests the
  ordinary collider chunks; wandering waits for its current chunk to load.

Bootstrap permits at most 24 herds of four (96 initial horses), with a hard
128-wild-horse world limit. Provisioned cavalry mounts have their own lifecycle
and do not consume this ambient limit. Habitat may produce fewer. These are explicit first-pass
budgets rather than an intended final wildlife density. No random camera-triggered
replacement, timed respawn or per-client population exists.

## Rendering

`client/src/animals/` owns presentation independently of humanoid appearance and
army controls. Horse snapshots use the same bounded interpolation math as people.
All horses share one GLB, material and animation graph. At most 32 nearby ambient
rigs exist, alongside a separate 160 mounted-rig budget; at most two scenes
instantiate per frame. Distance and zoom hysteresis
reduce churn at the boundary. Wide map views remove the rigs; returning rebuilds
them for the same replicated identities. Cavalry keep an authored static horse
proxy when their animated rig is absent. Offscreen animations have zero weight,
because pausing alone still makes Bevy evaluate the bones.

The supplied asset is unchanged: 318 source vertices, 616 triangles, 1,180 exported
vertices and a 21-bone skin. Idle, graze, alert, walk, trot, canter and gallop are
available; wild wandering currently uses walk. Horse clearance is 1.55 m, covering
the enlarged model's approximately 2.85 m nose-to-tail resting footprint.

## Next gameplay boundary

Mounted sword cavalry, wider formation spacing and synchronized rider animation
are available through the [cavalry battle lab](CAVALRY.md). `player/riding` owns
issued mounts and pair reconciliation; they do not wander or consume wildlife
population capacity. Stables, horse acquisition, breeding and taming remain future
work. No new player mounting UI was added for wild horses.
Wild horses are currently ambient animals, not combat targets or an economic input.

## Verification

```sh
cargo check --workspace --all-targets
cargo test --workspace
CITYSIM_MAP_ID=village_lab cargo test -p server meadow_population_uses_real_terrain_and_is_idempotent -- --ignored --nocapture
CITYSIM_MAP_ID=big_world cargo test -p server meadow_population_uses_real_terrain_and_is_idempotent -- --ignored --nocapture
BEVY_ASSET_ROOT="$PWD/client/assets" cargo run -p client --bin capture -- --scenario capture/scenarios/wild-horses.ron
```

The checked-in continuous capture stages four normal `Horse` roots through the
production animation consumer; it does not simulate wildlife on the client. PNG
metadata asserts both horse records and attached rigs.

For connected proof, build `cargo build -p server -p client --bins`, run the server
with `CITYSIM_MAP_ID=village_lab FISTWORLD_DEV=1`, then launch the client with:

```sh
CITYSIM_MAP_ID=village_lab BEVY_ASSET_ROOT="$PWD/client/assets" \
FISTFORCE_AUTOCONNECT=WildlifeReview FISTFORCE_START_FOCUS=-99,-1 \
FISTFORCE_START_ZOOM=24 FISTFORCE_AUTOTIME_PRESET=midday \
FISTWORLD_WILDLIFE_CAPTURE_DIR="$PWD/logs/captures/wildlife-connected" \
./target/debug/client
```

This opt-in client observes naturally spawned server animals, waits for actual
replicated grazing and movement, captures PNG/JSON evidence, zooms out to assert
zero rigs, restores the same identities, and exits. `behavior.json` records the
observed IDs and outcomes. A timeout or failed assertion exits unsuccessfully.
Keep these generated artifacts under ignored `logs/`.

Validation on 2026-09-09: the workspace check and 1,018 tests passed (18 existing
or explicit lab tests ignored). Explicit habitat audits additionally passed on
`village_lab` (39 horses) and `big_world` (81 horses). The connected lab observed
real grazing and movement, zero horse rigs at wide zoom and the same identities
on return. Its PNGs and metadata were inspected under
`logs/captures/wildlife-connected-final/`. The five-second offline scenario produced
26 successful PNG/JSON probes with all four production rigs present; the graze
loop and walking frames were inspected. These runs validate behavior and rendering,
not a measured frame-time improvement or connected cavalry.
