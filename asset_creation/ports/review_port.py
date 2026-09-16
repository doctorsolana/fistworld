"""Assemble an ignored water/shore review scene, render angles and a Bevy-only GLB.

Run with port.blend loaded. The canonical model and runtime assets are untouched.
Append -- render to render four inspection angles.
"""
import json
import math
import re
import sys
from pathlib import Path
import bpy
from mathutils import Vector
from mathutils.bvhtree import BVHTree

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[1]
sys.path.insert(0,str(HERE))
from build_port import export_glb, BuildingMesh, PALETTE, palette_material

scene=bpy.context.scene
OUT=HERE/'renders';OUT.mkdir(exist_ok=True)
port_objects=[o for o in bpy.data.objects if o.type=='MESH' and o.name.startswith('Port')]

# Check the physical route against actual triangles, not just nominal rectangles.
verts=[];faces=[]
for obj in port_objects:
    offset=len(verts);verts.extend(obj.matrix_world@v.co for v in obj.data.vertices)
    faces.extend(tuple(offset+i for i in p.vertices) for p in obj.data.polygons)
bvh=BVHTree.FromPolygons(verts,faces)
samples=[]
for x in (-1.2,0,1.2):
    samples.extend((x,-4.0+i*.20) for i in range(119))
for y in (17.0,18.1,19.0):
    samples.extend((-6+i*.20,y) for i in range(61))
for x,y in samples:
    hit=bvh.ray_cast(Vector((x,y,3.05)),Vector((0,0,-1)),3.1)
    assert hit[0] is not None and abs(hit[0].z-1.0)<.04,(x,y,hit[0])
# Cog envelope: no geometry intrudes into the alongside mooring box.
inside=[tuple(v) for v in verts if -4.5<v.x<4.5 and 21.0<v.y<24.2 and -1.0<v.z<5.2]
assert not inside,inside[:5]
(OUT/'geometry-checks.json').write_text(json.dumps({
    'clear_deck_samples':len(samples),'deck_height':1.0,
    'clear_main_lane_width':2.4,'cog_length':9.0,'cog_beam':3.2,
    'cog_envelope_clear':True,'scope':'Geometric checks only. No connected NPC or boat simulation.'},indent=2)+'\n')

def material(name, colour, rough=.8):
    m=bpy.data.materials.new(name);m.diffuse_color=(*colour,1);m.use_nodes=True
    b=m.node_tree.nodes.get('Principled BSDF');b.inputs['Base Color'].default_value=(*colour,1)
    b.inputs['Roughness'].default_value=rough
    return m

def box(name,center,size,mat):
    bpy.ops.mesh.primitive_cube_add(size=1,location=center);o=bpy.context.object;o.name=name
    o.dimensions=size;bpy.ops.object.transform_apply(location=False,rotation=False,scale=True)
    o.data.materials.append(mat);return o

sea=material('Review sea — not part of port',(.065,.22,.245),.32)
shore=material('Review shore — not part of port',(.30,.325,.245))
rock=material('Review shoreline stone',(.24,.275,.24))
box('Review water datum',(0,10,-.065),(110,110,.12),sea)
box('Review land',(0,-23,-1.02),(85,36,4),shore)
for i in range(20):
    x=-25+i*2.5
    if -8.6<x<6.6:continue
    bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=1,radius=1,location=(x,-4.95,-.20))
    o=bpy.context.object;o.name='Review shore rocks';o.scale=(1.50,1.10,1.16);o.data.materials.append(rock)

# A real 1.70 m shipped character is a scale reference, not asset contents.
before=set(bpy.data.objects)
bpy.ops.import_scene.gltf(filepath=str(ROOT/'client/assets/characters/Humanoid.glb'))
added=set(bpy.data.objects)-before
defaults=set(re.findall(r'default:\s*"([^"]+)"',(ROOT/'client/assets/characters/Humanoid.ron').read_text()))
for obj in added:
    if obj.parent is None:obj.location+=Vector((.60,11.6,1.0))
    if obj.name.startswith(('Top_','Bottom_','Hair_','Headgear_')):
        obj.hide_render=obj.name not in defaults;obj.hide_set(obj.hide_render)
    if obj.type=='ARMATURE':
        obj.animation_data.action=None
        for t in obj.animation_data.nla_tracks:t.mute=True
        for pb in obj.pose.bones:pb.matrix_basis.identity()
        idle=next((t.strips[0] for t in obj.animation_data.nla_tracks if t.name=='idle'),None)
        if idle:
            obj.animation_data.action=idle.action;obj.animation_data.action_slot=idle.action_slot
        obj.hide_set(True)

# Non-rendered volume makes ship clearance inspectable without inventing a ship asset.
cog=box('Reference — 9 m cargo cog clearance',(0,22.6,1.75),(9,3.2,5.5),rock)
cog.display_type='WIRE';cog.hide_render=True;cog.hide_set(True)
cog['Instructions']='Unhide to compare the docking berth with the current 9 m × 3.2 m cargo cog.'

# Review-only looping door action.
leaf=bpy.data.objects['PortOfficeDoor'];leaf.animation_data.action=None
for t in list(leaf.animation_data.nla_tracks):leaf.animation_data.nla_tracks.remove(t)
t=leaf.animation_data.nla_tracks.new();t.name='Door inspection'
for name,start in [('door_open',24),('door_close',95)]:
    strip=t.strips.new(name,start,bpy.data.actions[name]);strip.extrapolation='HOLD_FORWARD'
scene.frame_start=0;scene.frame_end=145;scene.frame_set(0)

scene.render.engine='CYCLES';scene.cycles.samples=24
scene.world.use_nodes=True;scene.world.node_tree.nodes['Background'].inputs[0].default_value=(.55,.66,.72,1)
scene.world.node_tree.nodes['Background'].inputs[1].default_value=.45
bpy.ops.object.light_add(type='SUN',location=(0,0,20));sun=bpy.context.object
sun.rotation_euler=(math.radians(28),math.radians(-20),math.radians(-35));sun.data.energy=2.3;sun.data.angle=.12
scene.render.resolution_x=1600;scene.render.resolution_y=1200;scene.render.resolution_percentage=100
scene.render.image_settings.file_format='PNG'
scene.view_settings.view_transform='AgX'

VIEWS={
    'harbour':((31,38,28),(-.4,7.8,1.8),37),
    'shore-office':((19,13,14),(-1.5,-.6,2.5),23),
    'dock-head':((-22,36,15),(0,17.6,1.6),26),
    'underside':((15,23,2.8),(0,12,.55),29),
}

def camera(view):
    loc,aim,scale=VIEWS[view];c=scene.camera;c.location=loc
    c.rotation_euler=(Vector(aim)-c.location).to_track_quat('-Z','Y').to_euler()
    c.data.type='ORTHO';c.data.ortho_scale=scale
    return Vector(aim)

aim=camera('harbour')
for screen in bpy.data.screens:
    for area in screen.areas:
        if area.type=='VIEW_3D':
            s=area.spaces.active;s.shading.type='MATERIAL';s.overlay.show_extras=False
            s.overlay.show_cursor=False;s.overlay.show_stats=True
            s.region_3d.view_location=aim;s.region_3d.view_distance=38
            s.region_3d.view_rotation=scene.camera.rotation_euler.to_quaternion();s.region_3d.view_perspective='PERSP'
bpy.ops.object.select_all(action='DESELECT')
bpy.context.preferences.filepaths.save_version=0
bpy.ops.wm.save_as_mainfile(filepath=str(OUT/'port-review.blend'))

# Bevy preview contains only the port plus the temporary shore/water staging.
# Lift its datum above the capture map's terrain; this is never a runtime building.
stage=[o for o in bpy.data.objects if o.type=='MESH' and not o.hide_render and (o.name.startswith('Port') or o.name.startswith('Review'))]
for obj in stage:obj.location.z+=6
export_glb(OUT/'PortReview.glb',stage,False)
for obj in stage:obj.location.z-=6

if 'render' in sys.argv:
    for view in VIEWS:
        camera(view);scene.render.filepath=str(OUT/(view+'.png'));bpy.ops.render.render(write_still=True)
camera('harbour')
print('PORT_REVIEW '+str(OUT),flush=True)
