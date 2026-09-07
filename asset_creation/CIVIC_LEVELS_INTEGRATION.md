# Civic hall assets and integration

Updated 6 September 2026. The three halls are built together by
`houses/build_civic_halls.py`, with facade primitives in `civic_mesh.py` and
windows, portals, dormers and belfries in `civic_details.py`.

## The progression

- **Moot Hall:** a substantial two-storey oak hall with boarded timber gables, terracotta courses, a carved
  civic shield and a copper-roofed bell cote.
- **Village Hall:** a stone ground floor supporting a jettied plaster-and-oak
  upper storey. Arched ground-floor windows, projecting joists and civic banners
  distinguish the middle level from the timber hall.
- **Town Hall:** three storeys of stone, dressed window arches, leaded glass,
  buttresses, a rose window, six roof dormers and a 21.42 m stone belfry/spire.

The same burgundy-and-gold shield, timber joinery, warm limestone and low-poly
roof courses connect the designs. Masonry faces are batched geometry; stones,
shingles and window surrounds are not separate mesh entities.

## Scale and exported cost

Dimensions are metres. Exported vertices include the splits needed for flat
normals and vertex colours; they are not Blender's welded vertex count.

| | Moot Hall | Village Hall | Town Hall |
|---|---:|---:|---:|
| Main wall plan | 6.00 × 7.60 | 6.50 × 8.80 | 9.20 × 13.35 |
| Wall height | 4.65 | 5.60 | 9.20 |
| Ridge height | 6.55 | 7.65 | 15.40 |
| Finial above terrain | 8.24 | 9.74 | 21.42 |
| Exported envelope X × Z | 7.19 × 8.78 | 8.15 × 10.10 | 10.39 × 14.53 |
| Reserved plot X × Z | 7.20 × 8.80 | 8.16 × 10.46 | 10.40 × 14.58 |
| Plot centre X, Z | 0, −0.20 | 0, 0.63 | 0, 2.69 |
| Previous exported vertices | 12,264 | 25,992 | 63,632 |
| New exported vertices | 10,931 | 13,596 | 40,433 |
| New triangles | 5,477 | 6,836 | 20,381 |

The previous main wall plans were 5.4 × 7.2, 6.0 × 8.4 and 9.0 × 13.0 m.
The new buildings therefore preserve their scale through their actual walls,
not merely through unchanged placement boxes. The village plot also preserves
the old rear extent. Foundations bed 20 cm into terrain; all sills remain at grade.

## Runtime contract

All three models face glTF −Z, use identity scale, and place the physical front
wall at Z = −4.02. `Anchor_Door` is always **(0, 0, −5.20)**, so civic upgrades
preserve the shared service approach and queues. Main walls grow behind the
frontage. Every plot keeps its front edge at Z = −4.60: even a few extra
centimetres can obstruct an in-progress departure during a hall upgrade. The
100-immigrant upgrade regression exercises that boundary. The 3 cm-high door bottom clears the 2 cm apron; there is no staircase.

`CivicHallLevel` chooses the current art; tier is the fallback for older roots.
City uses Town Hall until a fourth rung exists. The server's founding reservation
uses the largest supported hall, including its offset centre. Scene replacement
clears cached door and window-light wiring, then discovers the replacement nodes.

Each GLB contains three meshes: the body, `<Asset>Door`, and `<Asset>Glass`.
There are two materials (`CivicPalette`, `CivicHallGlass`), no textures, no skins,
and no glTF extensions. `door_open` lasts 16/24 s; `door_close` lasts 22/24 s.
Only the leaf and its attached hardware rotate. The authoring build checks the
whole 0–96 degree swing against the body at one-degree intervals.

`Light_Window.L` and `.R` locate two shadowless exterior lamps. Active settlement
roots drive the separate glass material after dark; ruins stay dark. Halls share
the existing nearest-40-buildings lighting budget with houses and workshops.
The belfry/interior anchors are retained for future use and create no extra lights.

Each hall uses one convex navigation hull, sliced near 2 m above grade:
`LowerYPercent` = (2.0 + 0.2) / total exported height. Roofs, belfries, dormers and
the upper jetty therefore do not widen the hull at walking height. The gameplay
shell/entrance path remains authoritative; this art change adds no room simulation.

## Rebuild and inspect

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup --threads 2 \
  --python-exit-code 1 --python asset_creation/houses/build_civic_halls.py
# Optionally append: -- --asset TownHall
python3 asset_creation/houses/inspect_prop_glb.py client/assets/game_assets/buildings/village/TownHall.glb
cargo build --release -p collider_baker --bin collider_baker_v2
target/release/collider_baker_v2
cargo check --workspace --all-targets
cargo test --workspace
```

The builder writes all canonical GLBs and the corresponding `moot_hall.blend`,
`village_hall.blend` and `town_hall.blend`. Studio elements are added after export;
Blender files open on a coloured inspection view with backface culling. The old
three builders were retired. **Do not use `export_prop_glb.py` on these files**:
its facing conversion belongs to older −X-authored assets.

Real Bevy scenarios are `civic-moot.ron`, `civic-village.ron`, `civic-town.ron`,
`civic-lineup.ron` and the continuous `civic-doors.ron`. See
[VISUAL-CAPTURE.md](../docs/VISUAL-CAPTURE.md) for the harness. Check PNGs and their
metadata, all sides, low eaves, day/night, and the complete door cycle. Offline
animation captures do not prove connected NPC traversal.

Regressions cover the anchor, clip targets/timing, geometry budgets, plot bounds,
full-size wall plans, level entrances, exposed roof undersides and panes in front
of the timber finish. Always inspect the window framing too: diagonal braces
must land on the frame below the sill, and banners must fit between window bays.
Chimney bases embed below the roof and sit inside the rear wall; a stack should
not poke through the gable below the shingles. Front shields mount over the king
posts with their coloured faces clear of the supporting timber.

## Reviewed in-game examples

Each PNG below has its unchanged `.capture.json` beside it. These are inspection
artifacts, not automatically accepted image-comparison baselines.

- [All three levels at the same scale](houses/renders/civic_lineup_day.png)
  (Town on the left, Village in the middle, Moot on the right).
- Moot Hall [front](houses/renders/civic_moot_day.png),
  [timber rear gable and inset chimney](houses/renders/civic_moot_rear.png), and
  [night lighting](houses/renders/civic_moot_night.png).
- [Village Hall](houses/renders/civic_village_day.png).
- Town Hall [day](houses/renders/civic_town_day.png),
  [night](houses/renders/civic_town_night.png), and
  [roof undersides](houses/renders/civic_town_underside.png).
- Continuous door cycle: [open](houses/renders/civic_doors_open.png) and
  [closed](houses/renders/civic_doors_closed.png).

The civic integration passed `cargo check --workspace --all-targets` and the
workspace suite (893 passed, 11 ignored). The 100-immigrant queue-through-upgrade
test passed after preserving the original forecourt boundary. Final captures
cover 30 PNG/JSON pairs across the five scenarios, with passing assertions.
All 15 building-asset regression tests passed again against the final timber-gable
exports using the freshly built shared test executable.
Collider auditing confirmed that only the three civic shapes changed compared
with the pre-civic database. No connected NPC traversal or frame-rate benchmark
is claimed by these offline visual checks.
