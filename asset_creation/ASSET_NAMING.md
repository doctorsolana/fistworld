# 3D asset names and registry

This is the naming authority for shipped 3D art. A model has one canonical
identity from its authoring file to its runtime path, shared enum, collider id,
display label, and internal GLB scene/node names.

## Convention

| Layer | Convention | Example |
|---|---|---|
| Blender source | descriptive `snake_case.blend` | `moot_hall.blend` |
| Runtime GLB | descriptive `UpperCamelCase.glb` | `MootHall.glb` |
| Rust variant | `UpperCamelCase` | `BuildingType::MootHall` |
| Serialized/collider id | `snake_case` | `building_moot_hall` |
| Display label | ordinary title case | `Moot Hall` |
| Variant suffix | `A`, `B`, `C` only for genuinely different meshes | `BirchA`, `BirchB` |

Do not put store-pack numbers, build techniques, version numbers, or words
such as `Graft` in a live asset identity. Those facts belong in source history.
Do not create two canonical prop kinds that point at one GLB.

Old serialized map ids are accepted only in `PropKind::from_id`. The loader
normalizes them immediately, so generated and authored content always write
the current id. This preserves existing maps without letting legacy vocabulary
spread into new code.

## Environment catalog

Every row below is registered exactly once in `shared::props::ALL_PROP_KINDS`.
The runtime tests assert that ids and paths are unique and that every file
exists.

| Family | Canonical files / variants |
|---|---|
| Small rocks | `SmallRockA`, `SmallRockB`, `SmallRockC` |
| Boulders | `BoulderA`, `BoulderB` |
| Broadleaf trees | `BroadleafNarrowA`, `BroadleafLargeA`, `BroadleafSpreadingA`, `BroadleafHighCrownA`, `BroadleafTallA`, `OakA`, `BirchA`, `BirchB`, `ChestnutA` |
| Dead trees | `DeadTreeA`, `DeadTreeB`, `DeadTreeC`, `DeadGnarledA` |
| Conifers | `PineA`, `PineB`, `PineTallA`, `PineTallB`, `PineYoungA`, `PineYoungB` |
| Bushes | `BushA`, `BushB`, `BushC` |
| Forest floor | `FernPatchA`, `FernPatchB` |
| Wildflowers | `FlowerA`, `FlowerB`, `FlowerC`, `FlowerD` |
| Grass | `GrassShortA`, `GrassTallA` |
| Work props | `WheatField`, `FishingPier` |

Creatures are not `PropKind`s (nothing scatters or paints them); they are spawned by the
systems that own them and live beside the environment art:

| Creature | File | Owner |
|---|---|---|
| Pasture sheep | `game_assets/environment/animals/Sheep.glb` | `LivestockPasture` visuals (client `settlement/mod.rs`), six per pasture, parts `Sheep`, `SheepHead`, `SheepLegFL/FR/BL/BR` posed by the client — no clips |

Tree paths are grouped by what they are:

```text
game_assets/environment/trees/
├── broadleaf/
├── conifer/
└── dead/
```

The previous `trees`, `trees_pine`, and `trees_dead` split is no longer used.

### Legacy map aliases

| Old ids | Canonical id |
|---|---|
| `rock_1`, `rock_4` | `small_rock_a` |
| `rock_2`, `rock_5` | `small_rock_b` |
| `rock_3` | `small_rock_c` |
| `tree_01` | `broadleaf_narrow_a` |
| `tree_02` | `oak_a` |
| `tree_08` | `broadleaf_large_a` |
| `tree_09` | `broadleaf_spreading_a` |
| `tree_10` | `birch_a` |
| `tree_18` | `chestnut_a` |
| `tree_29` | `broadleaf_high_crown_a` |
| `dead_tree_1..3` | `dead_tree_a..c` |
| `pine_tree_1`, `pine_tree_2` | `pine_a`, `pine_b` |
| `pine_tree_3` | `pine_tall_a` |
| `pine_tree_4` | `pine_young_a` |
| `bush_01`, `bush_04` | `bush_a` |
| `bush_02`, `bush_03` | `bush_b`, `bush_c` |
| `flower_01`, `flower_03` | `flower_a`, `flower_b` |
| `spring_flower_06`, `spring_flower_08` | `flower_c`, `flower_d` |
| `grass_patch`, `grass_tall` | `grass_short_a`, `grass_tall_a` |

The duplicate old ids deliberately resolve to the same canonical mesh. They
are compatibility input, not registered props.

## Buildings, character, resources, and tools

| Purpose | Authoring source | Runtime asset | Runtime identity |
|---|---|---|---|
| Generic humanoid | `character/humanoid.blend` | `characters/Humanoid.glb` + `Humanoid.ron` | `CharacterManifest` |
| Legacy humanoid donor | `character/humanoid_legacy_donor.blend` | not shipped | build input only |
| Log cabin | `houses/log_cabin.blend` | `buildings/village/LogCabin.glb` | `BuildingType::LogCabin` |
| Farmstead | `houses/farmstead.blend` | `buildings/village/Farmstead.glb` | `BuildingType::Farmstead` |
| Stone quarry/workshop | `houses/stone_quarry.blend` | `buildings/village/StoneQuarry.glb` | `BuildingType::StoneQuarry` |
| Church | `houses/church.blend` | `buildings/village/Church.glb` | `BuildingType::Church` |
| Livestock farm (sheep barn) | `houses/livestock_farm.blend` | `buildings/village/LivestockFarm.glb` | `BuildingType::LivestockFarm` |
| Moot Hall | `houses/moot_hall.blend` | `buildings/village/MootHall.glb` | `BuildingType::MootHall` |
| Village Hall | `houses/village_hall.blend` | `buildings/village/VillageHall.glb` | `BuildingType::VillageHall` |
| Town Hall | `houses/town_hall.blend` | `buildings/village/TownHall.glb` | `BuildingType::TownHall` |
| Carried goods | `resources/carried_resources.blend` | `resources/carried/*.glb` | item manifest |
| Work tools | `resources/work_tools.blend` | `tools/*.glb` | item manifest |
| Starter dinghy | `boats/dinghy.blend` | `vehicles/boats/Dinghy.glb` | boat mechanics entity |

The three civic levels have distinct runtime identities because each GLB has
its own footprint and baked collider. `SettlementBuildingKind::Hall` remains
their shared economic/administrative role.

## Adding or renaming an asset

1. Give the model a descriptive source filename and runtime filename.
2. Register one variant, canonical id, display label, and scene path in
   `shared/src/props/kinds.rs` or `shared/src/building/defs.rs`.
3. Add render/LOD classification and, if solid, one collider-manifest entry.
4. Update the world generator pools if the asset should occur there.
5. If old serialized content exists, add a read alias; never keep an alias in
   `ALL_PROP_KINDS`.
6. Normalize GLB metadata when a file was renamed:

   ```bash
   python3 asset_creation/normalize_glb_metadata.py --environment path/to/Model.glb
   ```

7. Rebuild `client/assets/colliders.bin` and run the workspace tests.

The collider baker rejects an id or path that disagrees with the shared
registry. The registry tests reject duplicate ids, duplicate paths, and missing
files, which catches the common partial-rename failures at review time.
