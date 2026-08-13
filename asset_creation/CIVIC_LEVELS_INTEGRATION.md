# Civic hall levelling — asset handover

Three hall assets are shipped, exported and integrated. This records the asset/runtime contract so a
later art or progression change does not accidentally break the permanent civic plot.

```
L1  MootHall.glb      hamlet    whole logs, shingle roof
L2  VillageHall.glb   village   stone ground floor, jettied half-timbered upper, shingle roof
L3  TownHall.glb      town      all stone, arcaded front, steep roof + dormers, belfry
```

> **A previous revision of this document was wrong** and the correction is worth stating up front,
> because the wrong version is the intuitive one. It built its runtime plan on `plot_building_rect`,
> which front-aligns a building to its plot's road edge. That code belongs to the **authored-city plot
> system**, which live simulated settlements do not use: `attach_settlement_visuals`
> (`client/src/settlement/mod.rs`) places the hall glb directly at the settlement position and always
> resolves it to `MootHall`. Verified. Everything below assumes the live path.

---

## 1. The asset invariant that makes upgrades safe — DONE

Because live settlements place the hall at the settlement position, swapping a bigger model in on the
same origin would move the visible door and frontage. `SettlementBuildingKind::door_offset(Hall)` is a
single hardcoded `Vec2::new(0.0, -5.20)` — the moot hall's anchor — so a bigger hall would also
invalidate the road endpoint, immigration and relief queues, permit collection and every cached route.

**Fixed in the asset, not in Rust.** `export_prop_glb.py` now translates every civic hall exactly
once so its `Anchor_Door` lands on one canonical local offset. It asserts both the anchor and the
physical façade position, preventing mesh and metadata from drifting apart. Measured from the
shipped files:

| | `Anchor_Door` (glTF X, Z) | front face Z | rear Z | shift applied |
|---|---|---|---|---|
| L1 | **(0.000, −5.200)** | −4.600 | +4.14 | −0.05 m |
| L2 | **(0.000, −5.200)** | −4.600 | +5.84 | −0.46 m |
| L3 | **(0.000, −5.200)** | −4.600 | +9.89 | −2.74 m |

Door identical on all three. **Front faces identical, spread 0.000 m.** Growth is backwards only
(4.14 → 5.84 → 9.89).

Consequences:

* `door_offset(Hall) = Vec2::new(0.0, -5.20)` **stays correct for every level.** No level-aware door
  lookup is needed; do not add one.
* A hall upgrade keeps the threshold, the frontage, the road endpoint and every queue position fixed.
* This is an asset invariant the exporter enforces, so it cannot drift out of sync with the art the
  way a table of per-level magic numbers would.

The models are still authored centred; the shift happens on export. The `.blend` files are unchanged
in that respect.

---

## 2. Per-level data, measured from the shipped `.glb`s

Origins are now the canonical ones from §1, so `local_center` is no longer near zero — it is the
footprint centre relative to the door-pinned origin.

| field | L1 MootHall | L2 VillageHall | L3 TownHall |
|---|---|---|---|
| `footprint` | `Vec2::new(6.4487, 8.7400)` | `Vec2::new(7.7779, 10.4400)` | `Vec2::new(10.2400, 14.4900)` |
| `local_center` | `Vec2::new(0.0, -0.2300)` | `Vec2::new(0.0, 0.6200)` | `Vec2::new(0.0, 2.6450)` |
| `height` | `8.1800` | `9.4400` | `21.6200` |
| `base_y` | `-0.1600` | `-0.1800` | `-0.2200` |
| door offset | `(0.0, -5.20)` | `(0.0, -5.20)` | `(0.0, -5.20)` |

Clearance: the existing 16 m hall clearance covers L3's 14.49 m depth, but only just, and it is
measured from a different origin now. **Reserve the largest supported civic footprint at founding**
rather than the current level's — that point from the earlier draft survives the correction.

---

## 3. Colliders — integrated

`BuildingType::VillageHall` and `BuildingType::TownHall` now exist. Their exact RON entries are live
beside the Moot Hall entry and the collider database has been rebuilt with:

```
cargo run --release --bin collider_baker_v2
```

`percent` is `(eaves - base_y) / height`, slicing the hull at the eaves so the roof overhang does not
inflate the footprint:

| | eaves | height | `LowerYPercent` |
|---|---|---|---|
| L1 | 4.40 | 8.18 | 0.558 *(already shipped)* |
| L2 | 5.06 | 9.44 | **0.555** |
| L3 | 9.10 | 21.62 | **0.431** |

Derived, not copied — L3 carries a 6.3 m roof and a 21.6 m belfry over a 9.1 m wall, so L1's 0.558
would swallow most of the roof and part of the tower. The same formula reproduces L1's existing 0.558
from its own numbers, which is how L2 and L3 were checked.

Baker currently green: 31 entries, `colliders.bin` rewritten.

---

## 4. Anchors and lights differ per level

`Anchor_Door` is on all three (§1). Everything else varies, so name lookups must tolerate absence:

| | L1 | L2 | L3 |
|---|---|---|---|
| other anchors | `Anchor_Notice` | — | — |
| lights | `Light_Interior`, `Light_Upper`, `Light_Window.L/R`, `Light_Belfry` | `Light_Ground`, `Light_Hall`, `Light_Hearth`, `Light_Upper.L/R`, `Light_Window.L/R`, `Light_Belfry` | `Light_Ground`, `Light_First`, `Light_Second`, `Light_Loggia`, `Light_Belfry` |

`Anchor_Notice` exists **only on L1**. A notice-board activity bound to it stops working at L2 as
"villagers stopped reading the notices" rather than as a missing-node error. `Light_Belfry` is on all
three, so a bell glow is safe to bind unconditionally.

**Door clips need no work:** all three ship `door_open` and `door_close`, same names, same single
animated node.

---

## 5. Runtime integration — completed 2026-08-11

Following the reviewed architecture, not the earlier draft:

1. **`CivicHallLevel`** is a serializable, replicated component on the settlement entity. Missing
   data derives from the settlement tier, with Hamlet/Ruins resolving to `MootHall`.
2. **`BuildingType::VillageHall` / `TownHall`** are separate variants. There is no `level: u8` inside
   `SettlementBuildingKind::Hall`; the semantic/art split in `actors.rs` is already right.
3. **The Hall entity is never replaced.** The settlement entity remains the hall and keeps the
   treasury, inventory, market, queues, jobs, policies, history and `SettlementId`; the level changes
   its visual, collider and ground claim in place.
4. **The live ladder is:** Hamlet → Moot, Village → Village, Town → Town, City → Town *(until a City Hall exists —
   do not rename `TownHall.glb`, it makes the later ladder confusing)*.
5. **Space is permanent:** settlement founding water checks, building permits and road surveys use
   the largest supported Hall footprint. F4 shows both the current and reserved shells.
6. **The construction seam remains:** the swap goes through `CivicHallLevel`, so a later
   treasury-funded project can keep the old hall operating and flip the level on completion.

Not asset concerns, but flagged by the review and worth keeping on the list:

* Ordinary houses should **not** all change on promotion — existing cabins preserve the town's history.
* Live thresholds are 4 / 12 / 24 residents, so automatic swapping gives a Village Hall at four
  residents. That wants resolving against the intended ~12 / ~30 before the progression reads right.

---

## 6. Known asset defects

Not blockers, but real and mine:

* **L3 has a 0.27 m² coplanar-face overlap** at the arcade piers (the log cabin, for comparison, is
  0.093 m²). It will flicker somewhere on the front at some camera angle.
* **L3 is 31,704 tris** against L2's 12,696, for a building ~1.4× larger in footprint. Most of it is
  rubble blocks and voussoirs that do not resolve at RTS distance. If there is a per-building budget
  this is the one that breaks it.
* **L2 is the weak rung visually** — within 1.3 m of L1 in height, so the middle step is carried almost
  entirely by wall colour at RTS zoom, while L2→L3 more than doubles.
* **The paved forecourt is not built.** When it is, it must be a **separate `.glb`**: inside
  `TownHall.glb` the single convex hull would swallow the paving and villagers would path around the
  square instead of across it.

---

## 7. Rebuild commands

```
blender --background --factory-startup --python asset_creation/houses/build_moot_hall.py
blender asset_creation/houses/moot_hall.blend --background --python asset_creation/houses/animate_door.py
blender asset_creation/houses/moot_hall.blend --background --python asset_creation/houses/export_prop_glb.py
```

Same three for `village_hall` and `town_hall`. Checks worth running after any change:

```
blender <asset>.blend --background --python asset_creation/houses/check_zfight.py     # coplanar faces
python3 asset_creation/houses/inspect_prop_glb.py <shipped>.glb                       # prop contract
blender <asset>.blend --background --python asset_creation/houses/preview_detail.py -- full
```
