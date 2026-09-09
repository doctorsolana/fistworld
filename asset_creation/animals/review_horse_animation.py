"""Assemble an ignored, playable Blender review from the canonical horse + shipped rider.
Run with horse.blend loaded. Space plays a wild-clip tour beside a trotting rider.
"""
import math
from pathlib import Path
import bpy
from mathutils import Vector

ROOT = Path(__file__).resolve().parents[2]
scene = bpy.context.scene
scene.render.fps = 60
horse = bpy.data.objects['HorseRig']
body = bpy.data.objects['Horse']
horse.animation_data.action = None
for pb in horse.pose.bones:
    pb.matrix_basis.identity()
# Clone only the horse instance; the approved mesh data remains shared.
ridden = horse.copy()
ridden.data = horse.data.copy()
ridden.name = 'RiddenHorseRig'
scene.collection.objects.link(ridden)
ridden.animation_data_clear()
ridden.animation_data_create()
ridden.location.x = 1.4
ridden_body = body.copy()
ridden_body.name = 'RiddenHorse'
scene.collection.objects.link(ridden_body)
ridden_body.parent = ridden
ridden_body.modifiers['Horse skin'].object = ridden
horse.location.x = -1.4

def strip(rig, action, start, repeat=1., scale=1.):
    rig.animation_data_create()
    track = rig.animation_data.nla_tracks.new()
    track.name = action.name
    result = track.strips.new(action.name, int(start), action)
    result.scale = scale
    result.repeat = repeat
    result.extrapolation = 'NOTHING'
    return result

frame = 1
for name in ['horse_idle','horse_graze','horse_alert','horse_walk','horse_trot','horse_canter','horse_gallop']:
    action = bpy.data.actions[name]
    repeat = 3 if name in {'horse_walk','horse_trot','horse_canter','horse_gallop'} else 1
    item = strip(horse, action, frame, repeat)
    scene.timeline_markers.new(name, frame=frame)
    frame = round(item.frame_end)
strip(ridden, bpy.data.actions['horse_trot'], 1, math.ceil(frame/42))
# Import the actual exported human, not a differently scaled studio copy.
bpy.ops.import_scene.gltf(filepath=str(ROOT/'client/assets/characters/Humanoid.glb'))
rider = next(o for o in bpy.data.objects if o.type == 'ARMATURE' and o not in (horse,ridden))
rider.animation_data.action = None
for track in list(rider.animation_data.nla_tracks):
    rider.animation_data.nla_tracks.remove(track)
for pb in rider.pose.bones:
    pb.matrix_basis.identity()
# Match the declared default wardrobe by node name.
import re
manifest=(ROOT/'client/assets/characters/Humanoid.ron').read_text()
defaults = set(re.findall(r'default:\s*"([^"]+)"', manifest))
# Stable current outfit from the manifest: default top/bottom/hair, no helmet.
print('Review wardrobe nodes:', [o.name for o in bpy.data.objects if o.name.startswith(('Top_','Bottom_','Hair_'))], flush=True)
for obj in bpy.data.objects:
    if obj.name.startswith(('Top_','Bottom_','Hair_','Headgear_')):
        show = obj.name in defaults
        obj.hide_render = not show
        obj.hide_set(not show)
scene.frame_set(0)
bpy.context.view_layer.update()
rider.location = ridden.matrix_world @ ridden.pose.bones['Anchor_Rider'].head
constraint = rider.constraints.new('CHILD_OF')
constraint.name = 'Follow authored rider seat'
constraint.target = ridden
constraint.subtarget = 'body'
constraint.inverse_matrix = (ridden.matrix_world @ ridden.pose.bones['body'].matrix).inverted()
strip(rider, bpy.data.actions['ride_trot'], 1, math.ceil(frame/42), .7)
scene.frame_start = 1
scene.frame_end = frame
scene.frame_set(1)
bpy.ops.object.select_all(action='DESELECT')
body.select_set(True)
bpy.context.view_layer.objects.active = body
horse.hide_set(True)
ridden.hide_set(True)
rider.hide_set(True)
for screen in bpy.data.screens:
    for area in screen.areas:
        if area.type=='VIEW_3D':
            space=area.spaces.active
            space.shading.type='MATERIAL'
            space.overlay.show_stats=True
            space.region_3d.view_perspective='PERSP'
            space.region_3d.view_rotation=Vector((-4,-7,-3)).to_track_quat('-Z','Y')
            space.region_3d.view_location=Vector((0,0,1.3))
            space.region_3d.view_distance=7.5
scene['Review instructions']='Space: play/pause. Left horse cycles named timeline clips; right horse and rider trot together. Canonical assets are unchanged by this review.'
bpy.ops.wm.save_as_mainfile(filepath=str(ROOT/'asset_creation/animals/renders/horse-animation-review.blend'))
