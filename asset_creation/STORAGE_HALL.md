# Storage Hall

Integrated on 2026-09-06. The private depot now uses authored low-poly art in
place of its blockout: oak framing, plaster gables, slate shingles, a loading
loft and hoist, covered cargo, and a hinged freight door.

![Storage Hall in Bevy](houses/renders/storage_hall_ingame.png)

## Cost and files

- **3,471 authoring vertices; 7,832 exported render vertices; 3,958 triangles.**
  Flat normals and per-face colours split vertices during export. Counts include
  the door and cargo, and exclude the source file's studio ground and lights.
- Two mesh primitives, one shared vertex-colour material, no textures or skins.
- `client/assets/game_assets/buildings/village/StorageHall.glb`: 309,524 bytes.
- Editable studio: `asset_creation/houses/storage_hall.blend`.
- Reproducible builder/exporter: `asset_creation/houses/build_storage_hall.py`.

## Integration contract

`BuildingType::StorageHall` retains the former blockout's binary discriminant
(13). The existing 9 × 7 m plot and semantic building/economy identity remain
unchanged. No checked-in authored RON map referenced the old blockout variant.
The scene and collider ID are now `StorageHall` / `building_storage_hall`.

Front faces glTF −Z; `Anchor_Door` and `Anchor_Work` are `(0, 0, -4)`.
The main door has a real opening and a separate hinge node, `StorageHallDoor`.
Its `door_open` clip swings outward 96 degrees in 0.667 seconds; `door_close`
returns to rest in 0.917 seconds. The existing `BuildingDoorDemand` consumer,
shared animation graph and hold/close state machine drive it. Interior cargo
is decorative; storage quantities and porter work remain server-owned.

The manifest bakes one 52-point convex hull below the roof/hoist. Its front is
at −3.62 m, leaving clearance for the 0.28 m character radius at the −4 m
entrance. The other 37 baked colliders remain geometrically identical.

## Rebuild and inspect

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
  --threads 2 --python asset_creation/houses/build_storage_hall.py
python3 asset_creation/houses/inspect_prop_glb.py \
  client/assets/game_assets/buildings/village/StorageHall.glb
cargo build --workspace --profile playtest
./target/playtest/collider_baker_v2
BEVY_ASSET_ROOT="$PWD/client/assets" ./target/playtest/capture \
  --scenario capture/scenarios/storage-hall.ron
BEVY_ASSET_ROOT="$PWD/client/assets" ./target/playtest/capture \
  --scenario capture/scenarios/storage-hall-door.ron
```

The first scenario checks front, entrance and rear views. The second holds a
single camera through 271 uninterrupted 60 Hz frames, photographing open,
closing, shut and reopening poses through the production animation systems.
All 13 PNG/JSON pairs passed their semantic assertions and representative
poses were inspected. These are actual offline Bevy game-renderer captures;
they do not assert that a connected NPC logistics journey was exercised.

The shared asset regression checks the plot, wire discriminant, door anchor,
matching animation durations/target, exported vertex budget and baked entrance
clearance. The existing server navigation suite covers rotated storage-hall
entrances with every other supported building kind.

![Open freight door in Bevy](houses/renders/storage_hall_door_open.png)
