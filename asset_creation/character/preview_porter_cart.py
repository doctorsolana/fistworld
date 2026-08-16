"""Porter + loaded cart, everything bound, correct loop range. Also checks the handles track.

    blender --background --factory-startup --python asset_creation/character/preview_porter_cart.py

Writes /tmp/porter_cart_loaded.blend -- open it and press space.

Two traps this exists to avoid, both of which look like broken assets and are not:

  * Humanoid.glb ships the WHOLE WARDROBE (4 bottoms, 4 tops, 6 hairstyles) in one file, because the
    game shows one per slot and hides the rest. Imported raw, all 14 render at once and it reads as
    several characters stacked and flickering.
  * The clip is frames 1..19 (19 duplicates 1, the loop seam) but the glTF import leaves the scene
    range at 1..250 -- so playback runs once and then holds the last pose for 231 frames, which looks
    like the animation has stopped.
"""
import bpy
from mathutils import Vector

for o in list(bpy.data.objects):
    bpy.data.objects.remove(o, do_unlink=True)
bpy.ops.import_scene.gltf(filepath="client/assets/characters/Humanoid.glb")
KEEP = {"Character_Base", "Bottom_Trousers", "Top_Jerkin", "Hair_Tousled"}
for o in [o for o in bpy.data.objects if o.type == 'MESH' and o.name not in KEEP]:
    bpy.data.objects.remove(o, do_unlink=True)   # the glb ships the whole wardrobe; wear one outfit

bpy.ops.import_scene.gltf(filepath="client/assets/game_assets/props/HandCart.glb")
for a, nm in zip(sorted([o for o in bpy.data.objects if o.name.startswith("Anchor_Load")],
                        key=lambda o: o.name), ["WoodBundle", "StoneBundle"]):
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=f"client/assets/game_assets/resources/carried/{nm}.glb")
    for o in [x for x in set(bpy.data.objects) - before if x.parent is None]:
        o.parent = a
        o.matrix_parent_inverse.identity()
        o.location = (0, 0, 0)

def bind(obj, name):
    act = bpy.data.actions.get(name)
    if not (obj and act):
        print(f"[prev] MISSING {name}")
        return
    obj.animation_data_create()
    obj.animation_data.action = act
    if hasattr(act, "slots") and act.slots:
        obj.animation_data.action_slot = act.slots[0]

arm = next(o for o in bpy.data.objects if o.type == 'ARMATURE')
body = next(o for o in bpy.data.objects if o.name.startswith("HandCartBody"))
bind(arm, "pull")
bind(body, "cart_pull")
for w in [o for o in bpy.data.objects if o.name.startswith("HandCartWheel")]:
    bind(w, "wheels_roll")

scn = bpy.context.scene
scn.frame_start, scn.frame_end = 1, 18
scn.render.fps = 24

grips = {o.name: o for o in bpy.data.objects if o.name.startswith("Anchor_Grip")}
worst = 0.0
for f in range(1, 19):
    scn.frame_set(f); bpy.context.view_layer.update()
    for side, gn in (("hand.L", "Anchor_GripL"), ("hand.R", "Anchor_GripR")):
        w = arm.matrix_world @ arm.pose.bones[side].head
        g = next((o for n, o in grips.items() if n.startswith(gn)), None)
        if g:
            worst = max(worst, (w - g.matrix_world.translation).length)
print(f"[prev] worst wrist-to-grip distance with the cart animated: {worst*100:.2f} cm")
print(f"[prev] (a constant 4.0 cm is BY DESIGN -- the shaft is set 3.0 below and 2.6 behind the")
print(f"[prev]  wrist joint so its axis runs through the palm, not the wrist bone)")
scn.frame_set(1)
bpy.ops.wm.save_as_mainfile(filepath="/tmp/porter_cart_loaded.blend")
print("[prev] saved")
