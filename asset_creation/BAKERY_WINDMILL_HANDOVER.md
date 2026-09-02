# Bakery, Windmill and Market — asset handover

> **Bakery and Windmill integrated 2026-08-12.** Both semantic building types now use the authored scenes and baked
> navigation hulls. Doors and night lights are live; bakery loaves reflect real Bread inventory;
> the mill cap faces upwind and its sails follow the shared wind speed only while the staffed,
> supplied business is on shift. Its cap now follows deterministic, slowly changing local wind
> direction throughout the day. The checklist below remains the exact art/runtime contract.
>
> **Market integrated 2026-08-15.** The earthen and paved scenes share one 12 × 12 m contract,
> upgrade in place through `MarketLevel`, remain walkable in both collision systems, and use the
> authored counter, trader and night-light anchors. See section 5a for the completed runtime seam.

Three village buildings, exported and contract-checked. All follow `PROP_PIPELINE.md`: authored
facing Blender −X, turned −90° about Z on export, so in game they face Bevy forward.

**The art is finished. Nothing below is a request to change art — it is the integration spec.**

```
Bakery.glb     7.04 x 8.24 m, height  5.33 m, base_y -0.16, eaves 2.20, ridge 4.55, flue 5.17
WindMill.glb   5.82 x 5.82 m, height 12.58 m, base_y -0.18, cabin head 2.20, cap 9.20, sails to 12.40
Market.glb       12.00 x 12.00 m, height 3.18 m, base_y -0.16   L1, beaten earth
MarketPaved.glb  12.00 x 12.00 m, height 3.18 m, base_y -0.16   L2, cobbled
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

## 1a. The Market — an OPEN-AIR square, and it does not fit the current blockout

> **2026-09-02: the market no longer ships a ground slab.** Both `Market.glb` and `MarketPaved.glb`
> are now stalls, poles, barrels and an EDGE only (timber edging with pegs for earthen, a dressed
> stone kerb for paved). The ground is the terrain, painted by the client the way roads are:
> `client/src/settlement/roads.rs` emits a rotated-rectangle paint op per market from its replicated
> position, rotation and `MarketLevel` — Dirt over the 12 x 12 plot when Earthen, Cobblestone over
> a dirt bed when Paved, with the roads' own strengths and falloffs. Square beds go down before every
> road and the cobble top after, so a stone road runs into the square as one surface. Paving is the
> same in-place repaint as a road upgrade; no server or protocol change. The paragraphs below that
> describe the slab, its joints, the dirt mottling and the "only the ground differs" rule are history.

**It ships as TWO LEVELS, and the only difference between them is the ground.** `Market.glb` is on
beaten earth — what a settlement has when it starts trading in a field — and `MarketPaved.glb` is the
same market once the ground has been paved. Every measurement, the stall layout, the anchors and the
canopies are byte-for-byte identical; one build script emits both (`-- 1` / `-- 2`).

That is deliberate and it differs from the hall ladder. A hall upgrade is a genuine reconstruction, so
Moot/Village/Town are three different buildings. Paving a square is paving a square: the upgrade has
to read as *the same place improved*, not as a different market dropped on the site. The kerb follows
the floor rather than the level number — L1 gets timber edging, because a dressed stone kerb around a
dirt floor is a detail that contradicts itself.

`MarketLevel` now selects the two scenes on the same authoritative market entity; see section 5a.

**Both are a paved/earthen market square with free-standing stalls, not a building.** An earlier version was a
covered timber market hall — an open arcade on posts under one pitched roof, which is what Llanidloes
and Chipping Campden are — and it was the wrong object: a hall is open at the *sides*, an open-air
market has no roof over it at all. The stalls' cloth canopies are the read at RTS distance, where a
shingle roof is just another shingle roof.

**It is 12.0 × 12.0 and the blockout reserves 9.0 × 7.0.** The square shape and the size were asked
for directly, so the Rust side has to move rather than the art. Everything below follows from that one
number:

| `BuildingDef` / constant | before integration | integrated value | why |
|---|---|---|---|
| `footprint` | `Vec2::new(9.0, 7.0)` | **`Vec2::new(12.0, 12.0)`** | the ground is exactly this, centred |
| `footprint_center` | `Vec2::ZERO` | **unchanged** | the model is centred on the origin |
| `height` | `3.2` | **unchanged** | measured 3.18 — it fits, just |
| `door_offset(Market)` | `Vec2::new(0.0, -4.0)` | **`Vec2::new(0.0, -6.5)`** | the edge is now at −6.0; −4.0 is *inside* the square |
| `clearance(Market)` | `11.0` | **raise to ≥ 13.0** | a 12 m square needs more than 11 m of ground |

`height` surviving is worth noting: the covered hall needed 4.68 m to get a village-pitched roof over
head height, and would have forced that constant to move. Canopies at 2.50 m and banner poles at
3.02 m come to 3.18 m total including the paving's −0.16 base, so 3.2 still holds.

`Anchor_Door` is authored **directly on** the new `(0.0, -6.5)`, so the exporter's door-pin applies a
zero shift — the pin would otherwise translate the model to place the door and move `footprint_center`
off zero to do it. `export_prop_glb.py`'s `CANON_DOOR_BY_STEM` already carries `-6.50` for the market,
so **the asset and the Rust constant now agree**, while the export keeps asserting the asset side so
future edits cannot silently move the road endpoint inside the square.

**Its collider should be `collidable: false`, not a hull.** This is the one building you are meant to
walk *into*. `collider_baker_v2` only offers a single `ConvexHull`, and the hull of a market square is
a solid 12 × 12 block — villagers would path around the whole thing, and the plaza, the stalls and the
paving would be decoration nobody can reach.

Layout: three stalls across the back, two on each flank, all facing a central plaza, with the whole
front left open so a visitor arriving at `Anchor_Door` walks into the square rather than into the back
of a stall. Two banner poles mark the front corners — with no roof the market has no silhouette from a
distance, and they give it one.

**Walkability is asserted at build time, not eyeballed.** The build flood-fills the square on a 0.25 m
grid at the villager's real 0.8 m `horizontal_radius` from the door approach and fails if any counter
has no reachable customer spot. That check earned its place immediately: the first layout stranded
**two of seven stalls** behind a 1.05 m pinch between the flank and back-corner stalls, and it looked
completely fine in every render. Current figure: **50.2 m² walkable, all seven counters reachable**
(51.8 m² before the stray barrels, which legitimately occupy 1.6 m²).

The same assert caught a barrel placed on the door approach and failed the build with *"the door
approach itself is blocked"* rather than shipping a market nobody could enter. If you move anything on
the ground, rebuild — do not eyeball it.

Each stall has 1.16 m of clear standing room behind its counter, which is why `Anchor_Trader` can
exist at all — an earlier version left 0.32 m and no villager could physically stand in it to serve.

Anchors, identical on both levels: `Anchor_Door` (0, −6.50), `Anchor_Counter` (0, +2.85, OUTSIDE in
the plaza where a customer stands), `Anchor_Trader` (0, +4.70, INSIDE behind the counter where the
seller stands), `Light_Interior`, `Light_Lantern` (on a banner pole).

**Only the DESIGNED structure is mirrored.** The symmetry assert covers the stalls, poles, paving and
the wear in front of each counter — an arrangement someone laid out, so mirroring it is right. Ground
mottling, loose stones, weeds and seven stray barrels and crates are added *after* the assert and are
deliberately asymmetric: they are things that happened to the market, and mirrored barrels read as
placed scenery, which is the one thing a stray barrel must not look like. Same split as the bakery's
oven corner. No door leaf and no door clip — an open market
has no door to swing, so a `door_open` lookup on this asset will warn.

> **Two bugs worth knowing if you ever edit the stalls.** There is no room for a back shelf: one sat
> at z 1.24–1.31, which is exactly villager eye height, so from the plaza it ran as a plank straight
> across the goods and across the face of whoever was serving. Below 1.0 fouls the counter, above 1.8
> fouls the canopy, and everything between is the sightline the stall exists to provide.
>
> And: They are built in a local frame where `v`
> runs *away* from the shopper. With that sign inverted — as it shipped once — the back shelf, the
> under-counter stock and the sacks all render on the customer's side of the counter, so a plank runs
> straight across the goods and the stallholder stands out in the street. If a stall looks like it has
> furniture in front of it, that sign is why.

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

## 5a. Market — integrated 2026-08-15

Follows the same shape as (a)–(c) above, with two deliberate differences flagged in section 1a.

The replicated `MarketLevel` selects between the two scenes. Village and earlier state uses Earthen;
Town and City use Paved. Both share `Anchor_Door` at `(0, -6.5)`, so switching levels moves nothing
and preserves the market entity, inventory, owner, workers and road relationships.

```rust
// shared/src/building/defs.rs -- rename in place, do NOT reorder
BuildingType::Market      => "building_market",        // was PlaceholderMarket
BuildingType::MarketPaved => "building_market_paved",  // new variant, append at the end
BuildingType::Market      => Some("game_assets/buildings/village/Market.glb#Scene0"),
BuildingType::MarketPaved => Some("game_assets/buildings/village/MarketPaved.glb#Scene0"),

// and on the Market's BuildingDef:
footprint: Vec2::new(12.0, 12.0),                 // was (9.0, 7.0) -- the market is square now
// footprint_center and height are already right: ZERO and 3.2. Do not change them.
```

Plus, outside `defs.rs`:

```rust
SettlementBuildingKind::Market => Vec2::new(0.0, -6.5),   // door_offset, was -4.0
SettlementBuildingKind::Market => 13.0,                   // clearance, was 11.0
```

The old market discriminant was renamed in place with a serde alias for `PlaceholderMarket`; the
new paved variant was appended. Storage halls no longer borrow market art and retain a separate
`PlaceholderStorageHall` blockout.

Both variants are in `ALL_BUILDING_TYPES` for model-contract coverage but explicitly report no baked
collider, and their manifest entries are `collidable: false`. The independent server rectangle
navigation cache also skips both variants; otherwise their 12 × 12 m footprints would still become
solid invisible walls despite the missing baked hull.

Construction levels a 3 m terrain apron beyond the visible 12 × 12 m floor, then blends back into
the biome over 1.8 m. The apron is intentional: terrain vertices are 2 m apart, so flattening only
to the asset edge lets an outside triangle interpolate through the slab. Completed markets loaded
from older saves are sampled once and re-levelled if they still carry the former 9 × 7 m terrace.

`door_offset(Market)` is `(0.0, -6.5)` and the asset is authored on it.

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
