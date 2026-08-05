# Asset handover — what is new and what the game must do with it

Everything below is **shipped and verified**: exported, contract-checked, colliders baked, workspace
compiles. Nothing here needs art work. What it needs is game code.

Verifiers, if you want to confirm any claim yourself:

```
python3 asset_creation/character/inspect_glb.py client/assets/characters/Humanoid.glb
python3 asset_creation/inspect_prop_glb.py client/assets/game_assets/buildings/village/MootHall.glb
```

> **Naming authority.** The current source is `asset_creation/character/humanoid.blend` and it ships
> as `client/assets/characters/Humanoid.glb` with `Humanoid.ron`. See `ASSET_NAMING.md` for the
> canonical catalog and legacy compatibility names.

---

## 1. Character — 4 new garments

`client/assets/characters/Humanoid.ron` now lists **14 items across 3 slots**:

| Slot | Index 0 | 1 | 2 | 3 |
|---|---|---|---|---|
| `bottom` | Bottom_Shorts | Bottom_Shorts_Long | **Bottom_Breeches** | **Bottom_Trousers** |
| `top` | Top_Tee | Top_LongSleeve | **Top_Jerkin** | **Top_Tunic** |
| `hair` | Hair_Tousled | Hair_Crop | Hair_Bowl | Hair_Spiky, Hair_Long, Hair_Afro |

Nothing to implement — `CharacterSlot::item(index)` already resolves these. Existing saved outfits are
unaffected because the new items were **appended**, never inserted.

> ### The one rule that matters here
> **These lists are APPEND-ONLY.** An outfit is replicated and persisted as a `u8` index per slot
> (`shared/src/components/actors.rs`) and resolved *positionally*. Reordering them silently redresses
> every existing villager — no error, just wrong clothes. If you ever need a different display order,
> sort at the UI layer, not in the data.

---

## 2. Character — 4 new body clips, and `walk` re-authored

`body_clips` is now **8**: `idle`, `walk`, `sit_down`, `sit_idle`, **`build`**, **`chop`**,
**`carry`**, **`talk`**.

| Clip | Frames | Duration | Loops | Use |
|---|---|---|---|---|
| `build` | 32 | 1.375 s | yes | hammering — construction |
| `chop` | 32 | 1.375 s | yes | felling a tree |
| `carry` | 18 | **0.75 s** | yes | walking with a load |
| `talk` | 72 | 3.0 s | yes | conversation |
| `walk` | 18 | **0.75 s** | yes | **re-authored, was 24 frames** |

**`carry`'s period is exactly `walk`'s.** Swap between them when a villager picks up or drops a
resource and the stride cadence does not change. Preserve normalised playback time across the swap
and the feet keep phase.

**`talk` is a BODY clip, not a face one.** This character's entire face is two eye rectangles — there
is no mouth to lip-sync, so speech is carried by gesture, head turns and weight shifts. That is the
right layer anyway: it composes with any of the five face clips, so a villager can argue angrily or
explain happily with no extra animation. Legs stay planted; it is a standing clip.

### ⚠ `walk` needs a matching client change, or it still slides

**This is the one item in this document that needs a code change rather than just wiring.**

`client/src/hero/mod.rs` normalises playback as:

```rust
let stride_speed = (visual.speed / HERO_MOVE_SPEED).clamp(0.4, 1.6);
```

That divisor asserts the walk clip is authored FOR `HERO_MOVE_SPEED` (3.2 m/s). It never was. Measured
ground speed, from the planted foot's travel per cycle:

| | old walk | **new walk** | carry |
|---|---|---|---|
| cycle | 1.00 s | **0.75 s** | 0.75 s |
| step | 0.305 m | **0.439 m** | 0.383 m |
| covers per cycle | 0.610 m | **0.879 m** | 0.765 m |
| implied ground speed | 0.61 m/s | **1.17 m/s** | 1.02 m/s |
| slip at 3.2 m/s | 5.24× | **2.73×** | 3.14× |

Re-authoring roughly doubled the covered ground (bigger stride, faster cadence), which is most of what
was visible as sliding. **The remaining 2.7× cannot be fixed in the animation.** Stride is capped by
leg length — hip 0.26 to foot 0.08 is 0.307 m scaled, so even a 45° swing only buys 0.87 m per cycle —
and closing the gap with cadence alone would need ~3.7 cycles/second, which is a blur, not a walk.

To remove the rest, divide by the clip's *authored* speed and raise the clamp:

```rust
/// Ground speed the walk clip is authored for, measured off the planted foot.
/// See asset_creation/ASSET_HANDOVER.md — do not guess this, re-measure if the clip changes.
pub const WALK_CLIP_SPEED: f32 = 1.17;

let stride_speed = (visual.speed / WALK_CLIP_SPEED).clamp(0.4, 3.0);
```

The `1.6` upper clamp must go up too, or it caps the correction at 1.6× and ~40% of the slip survives.

Worth flagging for design: **3.2 m/s is a sprint for a 1.7 m character** (a brisk human walk is about
1.4 m/s). At 2.7× playback the legs read as a fast jog. If villagers are meant to *walk*, lowering
`HERO_MOVE_SPEED` toward ~1.6–2.0 m/s would look considerably better than any animation can.

`talk` should be driven by dialogue state, not proximity — start it when a conversation starts, stop
it when it ends, and it will blend against `idle` the same way `build` already does in
`client/src/hero/mod.rs`.

`build` and `chop` are work rhythms, not one-shots — start them when work starts, stop them when it
stops. Neither has an impact event baked in; if you want a particle or sound on contact, the strike
lands at **phase 0.26** of the loop (frame 9 of 33), and the follow-through peaks at 0.33.

All three are full-body and loop exactly (verified frame-1-vs-frame-last delta = 0.000000).

---

## 3. Character — a new attachment joint for carried resources

The skin now has **17 joints**; the new one is **`attach.carry`**.

It weights **zero vertices** — it is a marker, not a deformer, and the skin is bit-for-bit unchanged.
No clip poses it, so it simply inherits the chest's motion.

**Rest transform in glTF space** (+Y up, −Z forward, metres, character 1.70 m tall):

```
attach.carry   position (0.000, 0.818, -0.463)   local +Y points world +Y (up)
               parent: torso
```

### How to attach a resource block

Find the joint entity by `Name` after the scene spawns, then parent the block to it:

```rust
// block sits in front of the chest, in both arms
commands.entity(attach_carry).with_child((
    SceneRoot(resource_block),
    Transform::from_xyz(0.0, block_height * 0.5, 0.0),   // seat its BASE on the joint
));
```

The `block_height * 0.5` offset is because the joint sits where the block's **base** should be.
Blocks between roughly **0.48 m and 0.58 m** land in the hands — much bigger covers the face, much
smaller and the hands float off its corners. Different resources should vary *material and proportion*
within that range rather than scale wildly.

Attach it **only while `carry` is playing**. In any other clip the arms are not around it.

> The load is carried **in front, in both arms** — not on the shoulder. A shoulder carry was built
> first and abandoned on measurement: this character is chibi-proportioned (head spans x ±0.19 against
> a shoulder joint at x 0.215), so a block where a human shoulder is renders *inside the skull*.

---

## 4. Four new buildings + a crop field

| Asset | `BuildingType` / `PropKind` | Footprint | Height | Collider |
|---|---|---|---|---|
| `LogCabin.glb` | `BuildingType::LogCabin` | 6.00 × 6.94 | 4.33 | hull, 45 pts |
| `LumberjackHut.glb` | `BuildingType::LumberjackHut` | 5.16 × 5.40 | 3.76 | hull, 60 pts |
| `Farmstead.glb` | `BuildingType::Farmstead` | 5.41 × 6.62 | 4.07 | hull, 51 pts |
| `MootHall.glb` | `BuildingType::MootHall` | 6.45 × 8.74 | 8.18 | hull, 57 pts |
| `FishermansHut.glb` | `BuildingType::FishermansHut` | 6.44 × 6.51 | 3.86 | hull, 71 pts |
| `WheatField.glb` | `PropKind::WheatField` | 8.00 × 11.00 | 0.86 | **none, by design** |
| `FishingPier.glb` | `PropKind::FishingPier` | 1.60 × 7.09 | 2.81 | **none, by design** |

All registered, all baked into `client/assets/colliders.bin`. Buildings live in
`game_assets/buildings/village/`; the field is in `game_assets/environment/crops/`.

Colliders are **convex hulls sliced at the eaves**, not decompositions — this is a top-down RTS and
what units need is a footprint to walk around, not 30 convex pieces describing roof steps.

> ### The wheat field deliberately has NO collider
> A crop must be walkable so farmers can stand in it to harvest. It has no `colliders_manifest.ron`
> entry at all — that absence *is* the declaration. It is also why the field is a separate asset from
> the farmstead: the baker gathers every mesh in a scene and `VertexFilter` has no by-name exclusion,
> so a field inside the building would bake as a solid 11 × 8 m block nothing could enter.
>
> `Farmstead.glb` ships an `Anchor_Field` empty marking where the field goes relative to the house, so
> the pairing lives in the asset rather than as an offset in Rust.

> ### The fishing pier is separate for the same reason
> Units must be able to **walk out along the deck**. A convex hull over a hut plus a 7 m jetty
> encloses every metre of open water between them — nobody could stand on it and units would path
> around a large invisible box floating on the sea. So the pier is `PropKind::FishingPier`, its own
> glb, with no manifest entry.
>
> `FishermansHut.glb` ships **`Anchor_Pier`** marking where the pier's LANDWARD END butts on; the
> pier's own origin is that landward end, at `z = 0 = SEA_LEVEL`, and it runs out along **+X**. Place
> it at the anchor with no rotation and it continues straight off the hut's seaward gable.
>
> **Its piles run to −1.05, below the waterline, and that is correct.** `SEA_LEVEL` is 0.0
> (`shared/src/worldgen.rs`). `inspect_prop_glb.py` reports a base that deep rather than failing it;
> the check that stayed hard is that nothing may FLOAT.
>
> Its own anchors: `Anchor_FishSpot` at the seaward end (where a fisherman stands to cast),
> `Anchor_Moor` at the waterline beside the bollard (where a boat ties up), and `Light_Lantern` in the
> lamp housing.

---

## 5. Anchors — named empties the game should read, not hardcode

Every building ships mesh-less nodes that Bevy spawns as named entities. Use them instead of offsets
in code; offsets silently go wrong the first time a building is resized, and nothing errors.

| Anchor | On | Meaning |
|---|---|---|
| `Anchor_Door` | all four | where a unit stands to enter — verified **outside** the collider |
| `Anchor_Work` | LumberjackHut | where a villager stands to chop at the block |
| `Anchor_Pier` | FishermansHut | where the pier's landward end goes |
| `Anchor_Nets` | FishermansHut | where a villager stands to mend nets |
| `Anchor_Field` | Farmstead | centre of the wheat field, 9.0 m off the door axis |
| `Anchor_Notice` | MootHall | where someone stands to read the board |
| `Light_Interior` | all four | room centre |
| `Light_Upper` | MootHall | first floor |
| `Light_Belfry` | MootHall | inside the bell cupola |
| `Light_Window.L` / `.R` | all four | just inside each pane |

Every stand-on anchor was measured against the baked hull. Current clearances, all outside:

```
LogCabin       Anchor_Door   +0.33      FishermansHut  Anchor_Door   +0.68
LumberjackHut  Anchor_Door   +0.63                     Anchor_Nets   +0.53
               Anchor_Work   +0.42                     Anchor_Pier   +0.11
Farmstead      Anchor_Door   +0.56      MootHall       Anchor_Door   +0.71
               Anchor_Field  +5.86                     Anchor_Notice +0.65
```

The fisherman's `Anchor_Door` needed moving to get there: at its first position it measured **0.26 m
INSIDE** its own hull, because the hull is CONVEX and the crates and floats sitting off to one side
drag the boundary forward across the entire front. **If you move a
building's props or its anchors, re-run that check** — a unit ordered to a point inside its own
building's collider will jam.

---

## 6. Door animations

All **five** buildings ship `door_open` (0.667 s) and `door_close` (0.917 s) as **node animations** on its
`*Door` mesh — no armature, no skin, entirely independent of the villager animation graph. One
`AnimationPlayer` on that entity plays one clip at a time.

They are a matched pair, not one clip reversed: `door_open` overshoots slightly, `door_close` bounces
off the jamb. Play without `.repeat()` so each holds its final pose.

**Drive by proximity with a hold, not per-unit:**

```
Shut → a unit within ~2 m wants to enter → play door_open
Open → hold while ANY unit is in range → clear for ~0.5 s → play door_close → Shut
```

Per-unit triggering breaks the moment two villagers arrive together: the second restarts `door_open`
on an already-open door and it snaps back to 0°.

Note the doorway is **solid** in the collider, as any hull's would be. Units path to `Anchor_Door` and
stop outside — correct while interiors are empty shells.

---

## 7. Window glow is game logic, and cannot be anything else

Each building ships its window panes as their **own node** (`CabinGlass`, `HutGlass`, `FarmGlass`,
`HallGlass`) with its own flat material, purely so you have something to write to. Raise `emissive` on
that material at dusk and drop it at dawn; put the light itself on the `Light_Window.*` anchors.

This is not a stylistic choice. **glTF animation channels can only target translation, rotation, scale
and morph weights** — material properties are not animatable in core glTF. Animating emissive is
`KHR_animation_pointer`, an extension this repo does not use and Bevy does not read. It also *should*
be game logic, since it depends on time of day and occupancy, which a baked clip cannot know.

---

## 8. Body/face animation layering — unchanged, but worth restating

Two layers compose at runtime through an `AnimationGraph` mask: put `eye.L`/`eye.R` in their own mask
group, mask them **out** of every body node and **in** on every face node. Then any of the 7 body clips
plays with any of the 5 face clips.

**The glb carries all 17 joints in all 12 clips regardless of what each clip authors.** Blender's
exporter emits every joint of an armature in every animation; that was verified twice and cannot be
turned off. It does not matter, because the mask blocks targets *at the graph node*, not by whether a
clip has curves for them. Do not try to "fix" this by editing the glb.

One useful consequence: since every clip drives every joint, the "an unkeyed bone holds the previous
clip's pose" failure cannot occur at runtime.

Give each villager a random time offset into its face clip, or a hundred of them blink in unison.

---

## 9. Two things to watch

**`Humanoid.ron` is generated.** It is emitted by `build_wardrobe_v2.py` and was rolled back to its
committed state once during this session — it lost both new bottoms, both new tops and all three new
clips while the `.glb` still contained them. Symptom: items or clips missing in game while the model
looks correct. `inspect_glb.py` cross-checks the manifest against the glb and catches it in a second.

**Re-baking colliders rewrites everything.** `collider_baker_v2` regenerates the whole of
`colliders.bin`. It also drops entries whose manifest rows are gone — five stale ones
(`building_train_station`, `desert_*`) were removed this way, all with zero references in code. Diff
before and after rather than trusting the byte count.
