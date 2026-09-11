# Asset creation index

Start here when editing art. Runtime files live under `client/assets/`; editable
sources and maintained generators live here. Read [PROP_PIPELINE.md](PROP_PIPELINE.md)
for buildings/props or [CHARACTER_PIPELINE.md](CHARACTER_PIPELINE.md) for characters.
[ASSET_NAMING.md](ASSET_NAMING.md) maps runtime names and identities.

Run commands from the repository root using your installed Blender executable.
A `build_*.py` script is not automatically interchangeable with another exporter:
current self-exporting buildings already face Blender +Y; the older prop exporter
rotates its supported sources. Follow the linked recipe for the asset being changed.

## Buildings and work props

| Runtime models | Editable source(s), relative to this directory | Maintained recipe |
|---|---|---|
| LogCabin, CabinL2, LongCabin, LongCabinL2 | `houses/log_cabin.blend`, `houses/cabin_l2.blend`, `houses/long_cabin.blend`, `houses/long_cabin_l2.blend` | `houses/build_houses.py`; [houses](HOUSE_HANDOVER.md) |
| MootHall, VillageHall, TownHall | `houses/moot_hall.blend`, `houses/village_hall.blend`, `houses/town_hall.blend` | `houses/build_civic_halls.py`; [civic ladder](CIVIC_LEVELS_INTEGRATION.md) |
| StorageHall | `houses/storage_hall.blend` | `houses/build_storage_hall.py`; [storage](STORAGE_HALL.md) |
| LumberjackHut | `houses/lumberjack_hut.blend` | `houses/build_lumberjack_hut.py`; [lumberjack](LUMBERJACK_HUT.md) |
| FishermansHut | `houses/fishermans_hut.blend` | `houses/build_fishermans_hut.py`; [fisherman](FISHERMANS_HUT.md) |
| WindMill | `houses/windmill.blend` | `houses/build_windmill.py`; [windmill](WINDMILL.md) |
| Bakery | `houses/bakery.blend` | `houses/build_bakery.py`; [bakery](BAKERY.md) |
| Tavern | `houses/tavern.blend` | `houses/build_tavern.py`; [tavern](TAVERN_PROCEDURAL_HANDOVER.md) |
| Farmstead, StoneQuarry, Church, LivestockFarm | Matching snake_case `.blend` files under `houses/` | Matching `houses/build_*.py`; [rural buildings](RURAL_BUILDINGS.md) |
| Market, MarketPaved | `houses/market.blend`, `houses/market_paved.blend` | `houses/build_market.py` plus the documented generic export path; [market recipe](BAKERY_WINDMILL_HANDOVER.md) |
| FishingPier, Sheep | `houses/fishing_pier.blend`, `houses/sheep.blend` | `houses/build_fishing_pier.py`, `houses/build_sheep.py` plus the supported generic prop export; [prop pipeline](PROP_PIPELINE.md) |
| WheatField | `houses/wheat_field.blend` | `houses/build_wheat_field.py`; [rural buildings](RURAL_BUILDINGS.md) |
| HandCart | `houses/handcart.blend` | `houses/build_handcart.py`; [porter cart](PORTER_CART_HANDOVER.md) |

Buildings export to `game_assets/buildings/village/`. Pier, sheep and wheat are
separate environment assets; the handcart lives in `game_assets/props/`.
The generic `houses/texture_and_light.py` and `export_prop_glb.py` remain required
for their supported older sources. Keep shared `building_mesh.py`, `civic_mesh.py`,
`civic_details.py` and `rural_architecture.py`; current builders import them.

After a building changes, regenerate and check its derived libraries following
[BUILDING_LODS.md](BUILDING_LODS.md). All nineteen canonical models and their
`game_assets/buildings/lod/` libraries ship. The runtime swaps meshes on existing
animated nodes; the libraries are not standalone building scenes.

Collision has one maintained baker: `collider_baker_v2`. Update the manifest and
run `cargo run -p collider_baker --bin collider_baker_v2` when collision inputs
change. Inspect the decoded result because the baker rewrites the entire pack.

## Characters, animals and equipment

| Family | Source and entry point | Contract |
|---|---|---|
| Humanoid and wardrobe | `character/humanoid.blend`; current animation, wardrobe, equipment and export steps | [character handover](CHARACTER_HANDOVER.md), [archery](ARCHERY_HANDOVER.md) |
| Horse | `animals/horse.blend`; `animals/build_horse.py` | [horse handover](animals/HORSE_HANDOVER.md) |
| Dinghy | `boats/dinghy.blend`; `boats/build_dinghy.py` | [dinghy handover](boats/DINGHY_HANDOVER.md) |
| Carried goods and work tools | `resources/carried_resources.blend`, `resources/work_tools.blend`; builders plus `resources/export_resources_glb.py` | [resource pipeline](resources/RESOURCE_PIPELINE.md), [tools](TOOL_HANDOVER.md) |
| Bow and arrow | `resources/bow.blend`; `resources/build_bow.py` starts from animated `character/humanoid.blend` | [archery handover](ARCHERY_HANDOVER.md) |
| Soldier sidearm | Procedural `resources/build_sidearm.py` is the complete geometry source | [archery handover](ARCHERY_HANDOVER.md) |
| Catapult | Procedural `siege/build_catapult.py` is the maintained geometry source | [catapult recipe](siege/README.md) |

Keep `character/humanoid_legacy_donor.blend`: it supplies geometry and animation
to the supported body/rig builders. The original raw character/vegetation imports
are not all available in this checkout. Preserve the cleaned editable sources and
canonical runtime GLBs; do not assume every model can be rebuilt from nothing.

## Vegetation, textures and UI

- [Vegetation pipeline](VEGETATION_PIPELINE.md) and [handover](VEGETATION_HANDOVER.md)
  identify the current procedural tools and retained imported models. Preview tools
  use the canonical environment catalogue under `client/assets/game_assets/`.
  Do not maintain a second copy of those GLBs in the authoring directory.
- [Meadow trees](MEADOW_TREES.md) use `vegetation/build_meadow_trees.py`, with editable
  `field_maple_a.blend`, `copper_beech_a.blend` and `wild_cherry_a.blend` sources.
- [Terrain textures](TERRAIN_TEXTURE_PIPELINE.md) retain the input PNGs under
  `terrain_source/`; `tools/terrain_ktx_builder` builds the runtime KTX2 arrays.
- `ui/build_hud.py` owns the HUD art. [Ledger art](ui/LEDGER-ART.md) describes
  procedural decorations, approved paintings, compression and model thumbnails.
  Retain compressed runtime artwork and fonts with their licences.
- `sound/gameintrosource.wav` is the editable music source; the game loads its
  compressed Ogg export. `branding/` contains the maintained emblem concept and prompt.

## Review output and verification

Generated screenshots, recordings, frame sequences and assembled Blender review
scenes belong in ignored `renders/` subdirectories or repository `logs/`.
Canonical editable `.blend` sources, generators, runtime files and capture RONs
belong in Git. Old experiments remain recoverable in Git history.

Use the real [Bevy capture harness](../docs/VISUAL-CAPTURE.md) for visual changes,
and inspect the PNG together with its `.capture.json`. Asset handovers link the
appropriate scenarios under `capture/scenarios/`; doors, windmills and streaming
changes require continuous captures. Offline rendering does not prove connected
NPC behavior. Run the client/server lab when traversal or combat changes.
