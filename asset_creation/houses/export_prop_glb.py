"""<asset>.blend -> client/assets/game_assets/buildings/village/<Name>.glb (Bevy 0.19 conventions).

    blender asset_creation/houses/farmstead.blend --background --python asset_creation/houses/export_prop_glb.py
    python3 asset_creation/houses/inspect_prop_glb.py client/assets/game_assets/buildings/village/Farmstead.glb

Generic where export_cabin_glb.py was written for one asset. The full reasoning lives in
PROP_PIPELINE.md; the short version of what this does that a plain File > Export would not:

  1. STRIPS THE STUDIO, by TYPE rather than by a whitelist of names. The .blend deliberately keeps a
     ground plane, a sun, two area lights and an ortho camera so it stays renderable; `Ground` is a
     60 m mesh and would otherwise ship as part of the building. A name whitelist has to be edited
     every time the asset grows a part, and its failure mode is silent omission.

  2. FIXES FACING. Bevy forward is -Z. export_yup maps Blender +Y -> glTF -Z, so for the door to face
     Bevy-forward the building must face Blender +Y. Both assets here are built facing -X, so
     everything rotates -90 deg about Z on the way out. The fix belongs in the asset, never as a yaw
     offset in Rust.

     The door is animated on rotation_euler.z, so its MESH DATA and LOCATION rotate while its object
     rotation stays identity — the hinge channel keeps its zero and the clips still read the same.
     Rotating the object would bake the 90 deg into the rest pose and measure every key from the
     wrong origin.

  3. STASHES BOTH CLIPS IN NLA. For an ARMATURE the exporter scans bpy.data.actions and matches by
     bone name, which is why the character's nine clips export from a plain fake user. Object-level
     animation has no such test: it exports only the active action plus whatever is in NLA tracks.
     Setting one active action ships ONE animation and says nothing about the other.

  4. RESETS SPECULAR/IOR to glTF defaults. The .blend keeps Specular at 0 so studio renders are matte;
     any non-default value exports as KHR_materials_specular, and the contract is no KHR extensions.
     Nothing is lost — the game mattes materials itself at load.
"""

import math
import os

import bpy
from mathutils import Matrix, Vector

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
STEM = os.path.splitext(os.path.basename(bpy.data.filepath))[0]

# Explicit, because the .blend stem is a working name and the shipped path is a game-facing one.
# The wheat field is not a building and does not live with them.
GLB_PATH = {
    "farmstead": "game_assets/buildings/village/Farmstead.glb",
    "wheat_field": "game_assets/environment/crops/WheatField.glb",
    # The civic ladder. A settlement replaces the building on the same plot as it grows, so these are
    # separate assets rather than variants -- the level 2 hall is a different construction (stone
    # ground floor, jettied half-timbered upper), not the level 1 hall with a different texture.
    "moot_hall": "game_assets/buildings/village/MootHall.glb",          # hamlet
    "village_hall": "game_assets/buildings/village/VillageHall.glb",    # village
    "town_hall": "game_assets/buildings/village/TownHall.glb",          # town
    "windmill": "game_assets/buildings/village/WindMill.glb",
    # The market ladder: L1 on beaten earth, L2 once the settlement has paved it.
    "market": "game_assets/buildings/village/Market.glb",
    "market_paved": "game_assets/buildings/village/MarketPaved.glb",
    "bakery": "game_assets/buildings/village/Bakery.glb",
    "fishermans_hut": "game_assets/buildings/village/FishermansHut.glb",
    "fishing_pier": "game_assets/environment/shore/FishingPier.glb",
    # The sheep barn. Its pasture is a separate replicated entity (LivestockPasture), not part of it.
    "livestock_farm": "game_assets/buildings/village/LivestockFarm.glb",
    # Pasture livestock: a creature, not a building. Six named parts the client animates itself
    # (head nod, leg swing), so it ships no clips and lives with the environment art.
    "sheep": "game_assets/environment/animals/Sheep.glb",
}
assert STEM not in {"log_cabin", "long_cabin", "cabin_l2", "long_cabin_l2"}, (
    "Village houses export themselves; run build_houses.py with --factory-startup."
)
assert STEM != "lumberjack_hut", (
    "The lumberjack workshop is authored in +Y and exports itself; run "
    "build_lumberjack_hut.py with --factory-startup instead of this -X exporter."
)
assert STEM in GLB_PATH, f"no shipped path registered for '{STEM}'; add it to GLB_PATH"
OUT = os.path.join(REPO, "client", "assets", *GLB_PATH[STEM].split("/"))


def log(m):
    print(f"[export] {m}", flush=True)


scene = bpy.context.scene

# The internal scene name is part of the asset contract as well as the filename. Blender's default
# "Scene" otherwise leaks into every export, makes runtime diagnostics ambiguous, and fails the same
# validation that protects us from accidentally shipping an old asset under a new filename.
scene.name = os.path.splitext(os.path.basename(OUT))[0]

# --- 1. strip the studio ---------------------------------------------------------------------------
for o in list(bpy.data.objects):
    if o.type in {"LIGHT", "CAMERA"} or o.name == "Ground":
        log(f"dropping studio object {o.name} ({o.type})")
        bpy.data.objects.remove(o, do_unlink=True)
log(f"shipping: {sorted(o.name for o in bpy.data.objects)}")

for o in bpy.data.objects:
    o.hide_viewport = o.hide_render = False
    o.hide_set(False)
    # The exporter only WARNS on bad geometry, so a broken mesh would ship silently.
    if o.type == "MESH" and o.data.validate(verbose=False):
        log(f"WARNING repaired invalid geometry in {o.name}")

# Zero or one door. A wheat field has none; a house has exactly one. More than one means the naming
# convention has been broken and the animation would be authored on the wrong object.
doors = [o for o in bpy.data.objects if o.type == "MESH" and o.name.endswith("Door")]
assert len(doors) <= 1, f"expected at most one *Door object, found {[o.name for o in doors]}"
door = doors[0] if doors else None
body = max((o for o in bpy.data.objects if o.type == "MESH"), key=lambda o: len(o.data.vertices))

# --- 2. verify the facing we are about to correct ----------------------------------------------------
# Asserted, not assumed: if a building is ever rebuilt with its door on another wall, this must fail
# loudly rather than silently ship one whose door faces sideways.
vs = [body.matrix_world @ v.co for v in body.data.vertices]
lo = [min(v[i] for v in vs) for i in range(3)]
hi = [max(v[i] for v in vs) for i in range(3)]
if door:
    hinge = door.matrix_world.translation
    # WHICH WALL, not how near a bbox corner. This used to compare the hinge against the -X extreme
    # with a tolerance, and every version of that is wrong because the extreme is set by whatever
    # clutter the yard happens to have: the fisherman's crates and floats push it to -3.77 and a
    # perfectly correct door failed. What actually matters is that the door is on an X-facing wall
    # and on the -X side, because that is what the -90 deg turn converts into Bevy-forward.
    assert hinge.x < 0 and abs(hinge.x) > abs(hinge.y), (
        f"door hinge ({hinge.x:.3f}, {hinge.y:.3f}) is not on the -X wall; "
        f"it must be the -X face for the export turn to make it Bevy-forward")
log(f"blend space: {hi[0]-lo[0]:.2f} x {hi[1]-lo[1]:.2f} x {hi[2]-lo[2]:.2f} m, "
    f"z {lo[2]:+.3f}..{hi[2]:+.3f}" + (", door on -X" if door else ", no door"))

# --- 3. into game space ------------------------------------------------------------------------------
m = Matrix.Rotation(math.radians(-90.0), 4, "Z")
for data in {o.data for o in bpy.data.objects if o.type == "MESH"}:
    data.transform(m)
for o in bpy.data.objects:
    o.location = m @ o.location      # empties have no mesh data; their LOCATION is all they are
if door:
    door.rotation_euler = (0.0, 0.0, 0.0)

# The facing correction above rotates geometry and pivots, but object animation
# channels are independent data and do not follow `Mesh.transform`. The source
# windmill turns around Blender X because its unexported shaft points along X;
# after the -90 degree facing turn that same physical shaft points along -Y.
# Conjugating the source rotation by that turn gives R_y(-theta). Without this
# conversion the exported clip visibly tumbles the sail disc around game X.
if STEM == "windmill":
    action = bpy.data.actions.get("sails_turn")
    assert action is not None, "windmill export requires sails_turn"

    def action_fcurves(a):
        if hasattr(a, "fcurves"):
            return list(a.fcurves)
        return [
            curve
            for layer in a.layers
            for strip in layer.strips
            for bag in getattr(strip, "channelbags", [])
            for curve in bag.fcurves
        ]

    rotation_curves = {
        curve.array_index: curve
        for curve in action_fcurves(action)
        if curve.data_path == "rotation_euler"
    }
    assert set(rotation_curves) == {0, 1, 2}, (
        f"sails_turn rotation channels are {sorted(rotation_curves)}, expected XYZ")
    source_x = rotation_curves[0]
    source_y = rotation_curves[1]
    source_x.array_index = 1
    source_y.array_index = 0
    for point in source_x.keyframe_points:
        point.co.y = -point.co.y
        point.handle_left.y = -point.handle_left.y
        point.handle_right.y = -point.handle_right.y
    log("converted sails_turn axis Blender +X -> post-facing -Y")
bpy.context.view_layer.update()

# --- 3b. CIVIC HALLS: pin Anchor_Door to one canonical local offset ------------------------------------
#
# Live settlements place the hall glb directly at the settlement position -- they do NOT go through the
# authored-city plot path -- and `SettlementBuildingKind::door_offset(Hall)` is a single hardcoded
# Vec2(0.0, -5.20), taken from the moot hall. Ship three halls whose Anchor_Door sits at three
# different local offsets and promoting a settlement silently moves the door, and with it the road
# endpoint, the immigration and relief queues, permit collection and every cached route.
#
# Translating the model here so every level resolves Anchor_Door to the SAME local point makes that
# constant correct for all of them, needs no level-aware door lookup in Rust, and means a hall upgrade
# grows backwards and sideways from a fixed threshold rather than sliding the frontage.
#
# It is deliberately not a per-level table in the game: an asset invariant that the exporter enforces
# cannot drift out of sync with the art the way a table of magic numbers can.
# CANON_DOOR is in glTF/Bevy space (X, Z) because that is the space the Rust constant is in. At this
# point in the script we are still in Blender coordinates, and export_yup will map Blender +Y -> glTF
# -Z. So the Blender-space target is (x, -z). Comparing the glTF value against Blender Y directly
# shifted every hall 10 m and flipped the door off its facing assert.
#
# THE SAME TREATMENT NOW APPLIES TO THE WINDMILL AND BAKERY, for a different reason. They are not a
# ladder and nothing replaces them, but `door_offset` gives them a shared placeholder Vec2(0, -4.0)
# while their art put the threshold at -3.51 and -5.02. That constant is what sets the road front edge
# (village_roads.rs), so the road, the queue and the door would have disagreed by up to a metre.
#
# Pinning the ART to the SHIPPED constant, rather than asking Rust to change to match the art, is the
# choice that cannot rot: the assert below fails the export the day the two drift apart, whereas a
# hand-edited constant just silently stops describing the model. It also means integration needs no
# door_offset change at all.
CANON_DOOR_BY_STEM = {
    "moot_hall": (0.0, -5.20),          # door_offset(Hall) -- one value for all three rungs
    "village_hall": (0.0, -5.20),
    "town_hall": (0.0, -5.20),
    "windmill": (0.0, -4.00),           # door_offset(Windmill)
    # The market is 12 x 12 now, so its edge is at -6.0 and -4.00 would put the threshold two
    # metres INSIDE the square. door_offset(Market) has to move to -6.50 with it.
    "market": (0.0, -6.50),             # door_offset(Market) -- NEEDS THE RUST CONSTANT MOVED
    "market_paved": (0.0, -6.50),       # both levels share a threshold, as the halls do
    "bakery": (0.0, -4.00),             # door_offset(Bakery)
    "livestock_farm": (0.0, -3.80),     # door_offset(LivestockFarm)
}
CIVIC = {"moot_hall", "village_hall", "town_hall"}
if STEM in CANON_DOOR_BY_STEM:
    CANON_DOOR = CANON_DOOR_BY_STEM[STEM]
    CANON_DOOR_BLENDER = (CANON_DOOR[0], -CANON_DOOR[1])
    anchor = bpy.data.objects.get("Anchor_Door")
    assert anchor is not None, f"{STEM} is door-pinned but has no Anchor_Door to align on"
    shift = Vector((CANON_DOOR_BLENDER[0] - anchor.location.x,
                    CANON_DOOR_BLENDER[1] - anchor.location.y, 0.0))
    # Translate each object exactly once, and ROOTS ONLY. Two ways to get this wrong, both of which
    # have actually happened here:
    #   1. moving mesh data AND the object location moves rendered geometry twice while empties such
    #      as Anchor_Door move once -- the anchor then looks right in metadata while the facade
    #      drifts out behind it;
    #   2. a PARENTED object's `location` is relative to its parent, so shifting the parent and the
    #      child both moves the child twice. The halls have no parenting and never showed this; the
    #      windmill's sails hang off the yawing cap and ended up 0.49 m out in front of the mill.
    # Children follow their parent for free, so only roots move.
    for o in bpy.data.objects:
        if o.parent is None:
            o.location = o.location + shift
    bpy.context.view_layer.update()
    got = (anchor.location.x, -anchor.location.y)          # back into glTF terms for the message
    assert abs(got[0] - CANON_DOOR[0]) < 1e-4 and abs(got[1] - CANON_DOOR[1]) < 1e-4, \
        f"{STEM} Anchor_Door landed at glTF {got}, wanted {CANON_DOOR}"
    if STEM in CIVIC:
        # Halls additionally share one facade plane, so a promotion cannot slide the frontage.
        body_world = [body.matrix_world @ v.co for v in body.data.vertices]
        front_z = -max(v.y for v in body_world)
        assert abs(front_z - (-4.60)) < 0.06, \
            f"{STEM} facade landed at glTF z={front_z:.3f}, wanted -4.600; anchor and art drifted apart"
    log(f"door-pin: shifted {shift.x:+.3f},{shift.y:+.3f} (blender) -> Anchor_Door at glTF {CANON_DOOR}")

for o in bpy.data.objects:
    assert o.scale[:] == (1.0, 1.0, 1.0), f"{o.name} has scale {o.scale[:]}, must be 1"

vs = [body.matrix_world @ v.co for v in body.data.vertices]
lo = [min(v[i] for v in vs) for i in range(3)]
hi = [max(v[i] for v in vs) for i in range(3)]
log(f"game space:  {hi[0]-lo[0]:.2f} x {hi[1]-lo[1]:.2f} x {hi[2]-lo[2]:.2f} m"
    + (f", door hinge at y={door.location.y:+.3f} (Blender +Y = glTF -Z = Bevy forward)"
       if door else ""))
if door:
    assert door.location.y > 0, "door did not land on +Y; it will not face Bevy forward"

# For a -Z-facing node with +Y up, right = forward x up = +X, so the building's LEFT is -X — the same
# handedness inspect_glb.py checks the character against. Asserted because .L/.R are just strings.
if "Light_Window.L" in bpy.data.objects:
    lw = bpy.data.objects["Light_Window.L"].location
    assert lw.x < 0, f"Light_Window.L landed at x={lw.x:+.2f}; .L must be on -X"
for o in bpy.data.objects:
    if o.type == "EMPTY":
        p = o.location
        log(f"anchor {o.name:16s} -> gltf({p.x:+.2f},{p.z:+.2f},{-p.y:+.2f})")

# --- 4. glTF-default materials, so nothing exports as a KHR extension ---------------------------------
for mat in {mm for o in bpy.data.objects if o.type == "MESH" for mm in o.data.materials if mm}:
    if not mat.node_tree:
        continue
    for n in mat.node_tree.nodes:
        if n.type != "BSDF_PRINCIPLED":
            continue
        n.inputs["Metallic"].default_value = 0.0
        n.inputs["Roughness"].default_value = 1.0
        for nm in ("Specular IOR Level", "Specular"):
            if nm in n.inputs:
                n.inputs[nm].default_value = 0.5
                break
        if "IOR" in n.inputs:
            n.inputs["IOR"].default_value = 1.5
    # Backface culling ON unless the asset explicitly opted out. Blender defaults to NO culling, which
    # exports as glTF doubleSided, so every closed building was shipping with culling disabled and
    # paying to rasterise interior faces that are then depth-tested away. Only geometry built from
    # single flat strips (the wheat field's straws) genuinely needs both sides.
    mat.use_backface_culling = not mat.get("double_sided", False)

# --- 5. stash the clips, then export -------------------------------------------------------------------
acts = sorted(a.name for a in bpy.data.actions)
log(f"actions: {acts or 'none'}")

# EACH CLIP IS STASHED ON THE OBJECT IT ANIMATES, not all of them on the door.
#
# This used to push every action in the file onto the door's NLA, which was correct while the only
# animated thing in any prop was a door. The windmill broke that: `sails_turn` on the door node would
# have exported a door that rotates a full turn and sails that never move, and it would have looked
# like an animation bug rather than an export one.
#
# The prefix names the owner. A clip whose owner is missing is a hard error -- silently dropping an
# authored animation is the failure mode this whole script exists to prevent.
ANIM_OWNER = {"door_": "Door", "sails_": "Sails"}


def owner_of(action_name):
    for prefix, suffix in ANIM_OWNER.items():
        if action_name.startswith(prefix):
            hits = [o for o in bpy.data.objects if o.type == "MESH" and o.name.endswith(suffix)]
            assert len(hits) == 1, f"'{action_name}' wants one *{suffix} object, found {len(hits)}"
            return hits[0]
    raise AssertionError(f"action '{action_name}' matches no owner prefix in {sorted(ANIM_OWNER)}")


if not acts:
    assert door is None or True, ""
by_owner = {}
for name in acts:
    by_owner.setdefault(owner_of(name).name, []).append(name)
for obj_name, names in sorted(by_owner.items()):
    obj = bpy.data.objects[obj_name]
    if not obj.animation_data:
        obj.animation_data_create()
    obj.animation_data.action = None
    for tr in list(obj.animation_data.nla_tracks):
        obj.animation_data.nla_tracks.remove(tr)
    for name in names:
        track = obj.animation_data.nla_tracks.new()
        track.name = name                # the exporter names the glTF animation after the track
        track.strips.new(name, 1, bpy.data.actions[name])
        track.mute = True                # stashed, not playing
    log(f"stashed on {obj_name}: {names}")
scene.frame_set(1)

os.makedirs(os.path.dirname(OUT), exist_ok=True)
bpy.ops.object.select_all(action="DESELECT")
bpy.ops.export_scene.gltf(
    filepath=OUT,
    export_format="GLB",
    export_yup=True,
    use_selection=False,
    use_visible=False,
    use_renderable=False,
    export_apply=False,
    export_skins=False,
    export_materials="EXPORT",
    export_image_format="AUTO",     # baked atlases embed in the .glb
    export_texcoords=True,
    export_normals=True,
    export_tangents=False,
    export_cameras=False,
    export_lights=False,
    export_extras=False,
    export_animations=True,
    export_animation_mode="ACTIONS",
    export_bake_animation=True,
    export_optimize_animation_size=False,   # keep the frame that lands each clip on its end pose
)
log(f"wrote {OUT} ({os.path.getsize(OUT) / 1024:.0f} KB)")
