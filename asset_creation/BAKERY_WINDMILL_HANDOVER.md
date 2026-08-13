# Bakery, Windmill and Market — asset handover

> **Bakery and Windmill integrated 2026-08-12.** Both semantic building types now use the authored scenes and baked
> navigation hulls. Doors and night lights are live; bakery loaves reflect real Bread inventory;
> the mill cap faces upwind and its sails follow the shared wind speed only while the staffed,
> supplied business is on shift. Its cap now follows deterministic, slowly changing local wind
> direction throughout the day. The checklist below remains the exact art/runtime contract.
>
> **The Market is new and is NOT integrated** — it still resolves to `PlaceholderMarket` with
> `model_path: None`. See section 1a; it needs two things the other two did not.

Three village buildings, exported and contract-checked. All follow `PROP_PIPELINE.md`: authored
facing Blender −X, turned −90° about Z on export, so in game they face Bevy forward.

**The art is finished. Nothing below is a request to change art — it is the integration spec.**

```
Bakery.glb     7.04 x 8.24 m, height  5.33 m, base_y -0.16, eaves 2.20, ridge 4.55, flue 5.17
WindMill.glb   5.82 x 5.82 m, height 12.58 m, base_y -0.18, cabin head 2.20, cap 9.20, sails to 12.40
Market.glb     9.00 x 7.00 m, height  4.68 m, base_y -0.16, eaves 2.30, ridge 4.32   <- NOT yet integrated
```

Verify either yourself:

```
python3 asset_creation/houses/inspect_prop_glb.py client/assets/game_assets/buildings/village/Bakery.glb
python3 asset_creation/houses/inspect_prop_glb.py client/assets/game_assets/buildings/village/WindMill.glb
```

> **`inspect_prop_glb.py` was fixed as part of this work and you need the new version.** It used to
> add each node's own translation only, ignoring the parent chain, so the windmill's sails — a child
> of the yawing cap — were measured in the cap's local space and reported as reaching 3 m
> *underground*. Real base is −0.18. The same bug could have failed a correct asset for "floating", so
> re-run anything you checked with the old version.

---

## 1. Road alignment — already correct, do NOT change `door_offset`

`door_offset` gives both kinds a shared `Vec2::new(0.0, -4.0)`, and it is what sets the road front
edge ([`village_roads.rs:1301`](../server/src/world/village_roads.rs#L1301)). The art originally put
the thresholds at −5.02 and −3.51, so the road, the queue and the visible door disagreed by up to a
metre.

**Fixed in the asset, not in Rust.** `export_prop_glb.py` now translates each building on export so
its `Anchor_Door` lands exactly on the shipped constant, and asserts it:

| | `Anchor_Door` (glTF X, Z) | shift applied | `footprint` | `local_center` |
|---|---|---|---|---|
| Bakery | **(0.000, −4.000)** | −1.02 m | `Vec2::new(7.0420, 8.2400)` | `Vec2::new(0.0990, 0.7200)` |
| WindMill | **(0.000, −4.000)** | +0.49 m | `Vec2::new(5.8180, 5.8180)` | `Vec2::new(0.0000, -0.4910)` |

Pinning the art to the constant rather than editing the constant to match the art is deliberate: the
export **fails** the day the two drift apart, whereas a hand-edited constant just quietly stops
describing the model. The civic halls already work this way; the table now covers all five.

`WindMill`'s footprint above is the **solid** building. Its full glTF bounds are 8.80 m across because
the sail disc sweeps ±4.40 m in X, well outside the walls. Use 5.82 for plots and clearance, and mind
the sweep for anything that cares about overhead space. The disc turns in the plane Z = −2.911,
inboard of the porch tip at −3.400 and 3.6 m up, so it clears the building at every yaw angle.

> **`export_prop_glb.py` also had to be fixed for this**, and the bug is worth knowing about because
> it is silent. The door-pin translates every object by one offset — but a *parented* object's
> `location` is relative to its parent, so shifting parent and child both moved the child **twice**.
> The three halls have no parenting and never showed it; the windmill's sails hang off the yawing cap
> and landed 0.49 m out in front of the mill. Roots only now. If you add parenting to any other prop,
> this is the trap.

## 1a. The Market — built to the reserved footprint exactly, and two things it needs from Rust

Unlike the other two, the market had a blockout to fit: `BuildingType::PlaceholderMarket` reserves
`footprint: Vec2::new(9.0, 7.0)` with `footprint_center: Vec2::ZERO`. The shipped glb is **exactly**
9.00 × 7.00 and centred on the origin (X −4.500..+4.500, Z −3.500..+3.500), so both fields stay
correct as written. Getting there needed three things clamped rather than trusted — shingle course
jitter, seam slop and the verge boards each pushed a few centimetres past the edge, and a first pass
with a projecting awning came out 7.70 m deep. A building wider than the ground the settlement
reserved for it will clip whatever is placed next door.

`Anchor_Door` is authored **directly on** `door_offset(Market)` = `(0.0, -4.0)`, so the exporter's
door-pin applies a zero shift. That matters here specifically: the pin would happily translate the
model to fix the door, and would have moved `footprint_center` off zero to do it.

**Two changes it needs beyond the usual:**

**`height` must go from 3.2 to 4.68.** 3.2 cannot be met honestly. The eaves alone have to clear head
height on a building people walk under (2.30 m here), and any roof pitched like the rest of the
village then adds ~2 m over a 3.5 m half-span. A 3.2 m ridge means either a nearly flat roof, which
belongs to no other building in this settlement, or eaves at 1.6 m, which a villager cannot walk
beneath.

**Its collider should be `collidable: false`, not a hull.** This is the one building in the set you
are meant to walk *into* — that is what "open-air market" means, and the arcade has an entrance on all
four sides. `collider_baker_v2` only offers a single `ConvexHull`, and the convex hull of an open
arcade is a solid 9 × 7 block: villagers would path around the market rather than through it, and the
stalls, paving and through-route would all be decoration nobody can reach. Ship it uncollidable, or
leave it out of the manifest entirely.

Anchors: `Anchor_Door` (0, −4.00), `Anchor_Counter` (0, −3.10, the through-route centre where a trader
stands), `Light_Interior`, `Light_Lantern`. No door leaf and no door clip — an open market has no door
to swing, so a `door_open` lookup on this asset will warn and should not be attempted.

The design follows the surviving halls (Llanidloes, Chipping Campden, the Titchfield hall at the Weald
& Downland museum), which are all the same building: an open arcade on timber posts standing on stone
plinths, divided into bays, *panelled up about breast high with an entrance on each side*, under a
pitched roof whose carpentry is visible from underneath. Two of those turned out to be load-bearing —
without the breast-high boarding a roof on bare posts is a bandstand, and this is the only building
here whose roof is read from below, so the shingle steps need purlins and rafters under them.

## 2. `Stock_Bread_1..6` — the one node contract unlike any other building

The bakery's loaves are **six separate mesh nodes**, not part of the building mesh, so a bakery with
an empty larder can look empty:

| show | meaning |
|---|---|
| `Stock_Bread_1 .. Stock_Bread_N` | stock level `N/6`; hide `N+1 .. 6` |
| none | out of bread |
| all six | full |

Ordered deliberately:

* **In mirrored pairs** — `(1,2)`, `(3,4)`, `(5,6)`. Any *even* N stays left/right symmetric.
* **By prominence** — 1–4 are on the counter nearest the door, 5–6 on the low back shelf, so selling
  out empties the back first.

60 tris each, 360 total, so hiding them saves real geometry at RTS zoom as well as telling the truth.

## 3. Anchors and lights

| | Bakery | WindMill |
|---|---|---|
| anchors | `Anchor_Door` (0, −4.00), `Anchor_Counter` (0, −3.95) | `Anchor_Door` (0, −4.00) |
| lights | `Light_Interior`, `Light_Lantern`, `Light_Oven` | `Light_Interior`, `Light_Lantern`, `Light_Tower` |

`Anchor_Counter` is where a customer stands to be served — centred on the doorway gap and outside the
awning posts, so a queue forms in front of the shop rather than inside it.

**Every light path in the client is gated on building kind.** The runtime now binds both buildings'
`Light_Interior` and `Light_Lantern` anchors through the shared workplace-night-light system:

* `Light_Lantern` — bound at [`settlement/mod.rs:970`](../client/src/settlement/mod.rs#L970), currently
  only for `FishingPierVisual` (520 000 lm, range 8.5).
* `Light_Interior` — [`settlement/mod.rs:975`](../client/src/settlement/mod.rs#L975), currently only for
  `FishermansHut` (820 000 lm, range 10.0).
* `Light_Window.L/R` — the emissive-pane system, gated on `With<Household>` **and** a material named
  `CabinGlass`. Neither building is a household and their glass is `BakeryGlassDark` /
  `WindMillGlass`, so this path does not apply. Not shipped on either building.

Both buildings use the two established names above so wiring them is a match arm and nothing else.
`Light_Oven` (inside the oven mass) and `Light_Tower` are extras with no binding yet — inert, not
broken.

`Light_Oven` is deliberately **inside** the building. There is no exterior firebox: a bakery's oven
mouth is indoors. What identifies the building from outside is the masonry corner and the flue.

## 4. Animation clips — present, correct and integrated

| clip | node | frames |
|---|---|---|
| `door_open` / `door_close` | `BakeryDoor`, `WindMillDoor` | 16 / 22 |
| `sails_turn` | `WindMillSails` | 49, LINEAR |

Doors work through the shared building-door graph, which looks the clips up by exactly these names.

`sails_turn` is consumed by the windmill motion binding. It is a seamless loop: frame 49 is 360°,
the same pose as frame 1. Playback speed follows the same deterministic wind swell as the clouds,
scales with master time warp, and stops when the mill is not staffed, supplied and on shift.

`WindMillSails` is a child of `WindMillCap`; the runtime continuously yaws the cap so the authored
-Z front faces a slowly changing local wind direction after accounting for the permanent plot
rotation. The sail sweep is asserted clear of the cabin at 48 yaw angles, so any yaw angle is safe.
The clip itself does not reverse when the wind changes compass bearing: like a real yawing mill,
the whole cap turns into the wind while the rotor retains its local mechanical direction.

The source action correctly turns around the Blender-space windshaft (X). `export_prop_glb.py`
conjugates that channel through its -90-degree facing correction, producing rotation around the
exported game-space windshaft (Z). `inspect_prop_glb.py` reads the actual animation quaternions and
rejects a windmill whose sail axis is anything else; this caught and corrected the original export,
which rotated the mesh but left its animation channel on X.

## 5. Integration checklist

The Bakery and Windmill are **done** — runtime integration and the exporter-side sail-axis conversion
are complete, and (a)–(c) below are the record of what was changed. **The Market is not**; its
outstanding work is section 5a.

**a. `shared/src/building/defs.rs` — give the variants real art.** Rename in place; do **not**
reorder, the file notes that replicated discriminants must stay stable.

```rust
BuildingType::Windmill => "building_windmill",     // was PlaceholderWindmill / "placeholder_windmill"
BuildingType::Bakery   => "building_bakery",       // was PlaceholderBakery   / "placeholder_bakery"

// scene_path(): move them out of the None arm
BuildingType::Windmill => Some("game_assets/buildings/village/WindMill.glb#Scene0"),
BuildingType::Bakery   => Some("game_assets/buildings/village/Bakery.glb#Scene0"),
```

`has_baked_collider()` is `scene_path().is_some()`, so this switches colliders on too. Also update the
two call sites in [`village_roads.rs:2020-2021`](../server/src/world/village_roads.rs#L2020) and
[`actors.rs:433-434`](../shared/src/components/actors.rs#L433).

**b. Add both to `ALL_BUILDING_TYPES`** in the same file, or `collider_baker_v2` will not see them.

**c. `client/assets/colliders_manifest.ron`** — `percent` is `(eaves − base_y) / height`, slicing the
hull at the eaves so the roof does not inflate the footprint:

```ron
(
  kind: "building_bakery",
  gltf_path: "game_assets/buildings/village/Bakery.glb#Scene0",
  mode: ConvexHull,
  vertex_filter: LowerYPercent ( percent: 0.443 ),
  collidable: true,
),
(
  kind: "building_windmill",
  gltf_path: "game_assets/buildings/village/WindMill.glb#Scene0",
  mode: ConvexHull,
  vertex_filter: LowerYPercent ( percent: 0.189 ),
  collidable: true,
),
```

The bake was regenerated with `cargo run --release --bin collider_baker_v2`. The windmill's 0.189 is low **on purpose**: the
hull must stop at the cabin head, or a single convex hull swallows a 8.8 m sail disc and villagers
path around empty air.

**d. Lights** — add the two kinds to the match arms in §3. Both anchors already exist.

**e. Sails and cap** — play `sails_turn` on `WindMillSails` scaled by wind speed, and continuously
set `WindMillCap`'s Y rotation from wind direction. Nothing else is needed on the art side. Future
exports must preserve the exact names, keep `WindMillSails` parented to `WindMillCap`, keep the cap
pivot on the tower's vertical axis, and keep the sails pivot centred on their hub.

**f. Bread** — bind `Stock_Bread_1..6` visibility to the bakery's stock per §2.

`door_offset` needs **no change** — see §1.

## 5a. Market — outstanding

Follows the same shape as (a)–(c) above, with two deliberate differences flagged in section 1a.

```rust
// shared/src/building/defs.rs -- rename in place, do NOT reorder
BuildingType::Market => "building_market",        // was PlaceholderMarket / "placeholder_market"
BuildingType::Market => Some("game_assets/buildings/village/Market.glb#Scene0"),
```

On the Market's `BuildingDef`, `height` must go **3.2 → 4.68** (measured; see section 1a for why 3.2
is unreachable). `footprint: Vec2::new(9.0, 7.0)` and `footprint_center: Vec2::ZERO` are already
correct and the asset was built to them exactly — do not change either.

Also update the `PlaceholderMarket` call sites at
[`village_roads.rs:2020-2021`](../server/src/world/village_roads.rs#L2020) and
[`actors.rs:560`](../shared/src/components/actors.rs#L560).

**Do NOT add it to `ALL_BUILDING_TYPES` expecting a hull.** `has_baked_collider()` is
`scene_path().is_some()`, so giving it a scene path switches collider baking on — and a single
`ConvexHull` over an open arcade is a solid 9 × 7 block. Villagers would path *around* the one
building in the village they are supposed to walk *through*. Ship it `collidable: false` or leave it
out of the manifest.

`door_offset(Market)` is already `(0.0, -4.0)` and the asset is authored on it — no change.

There is no `door_open`/`door_close` on this asset and there should not be: an open market has no door.

## 6. Known limits

* **Bakery symmetry is checked on 2760 of 3744 verts.** The oven corner and its mirror are exempt: the
  oven cannot be mirrored, and the cabin is cut back around it, so that corner is genuinely
  asymmetric. Everything else still passes at 1e-6, and the build prints the exempt count every run.
* **Bakery has ~750 coplanar face pairs**, largest 0.030 m² (the log cabin is 0.093 m² in total).
  Nothing above 0.01 m² is on a visible face. Re-check with `check_zfight.py`.
* **The oven is invisible from the shopfront.** By construction — it sits on the rear corner so it does
  not compete with the awning. From the front the only cue is the flue over the ridge.

## 7. Rebuild

```
blender --background --factory-startup --python asset_creation/houses/build_bakery.py
blender asset_creation/houses/bakery.blend --background --python asset_creation/houses/animate_door.py
blender asset_creation/houses/bakery.blend --background --python asset_creation/houses/export_prop_glb.py
```

Same for `windmill`, plus `animate_sails.py` between the door pass and the export.
