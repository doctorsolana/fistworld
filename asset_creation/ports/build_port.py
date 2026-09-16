"""Standalone harbour art study. Never writes to client/assets or game code.

Blender --background --factory-startup --python build_port.py
Metres; +Y seaward in Blender, -Z in GLB. Origin is the shore water datum.
All walking decks are at +1.0 m, matching the surveyed port's pier-end height.
The broad T-head is a proposed art layout, not the current procedural port contract.
"""
import json
import math
import struct
import sys
from pathlib import Path

import bpy
from mathutils import Vector

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(ROOT / "asset_creation/houses"))
from building_mesh import BuildingMesh, palette_material, animate_door, roof_underside
from civic_mesh import Wall, shingle_roof, lathe, masonry, segments
from civic_details import window, lantern, validate_swing
from rural_architecture import PALETTE as BASE, reset, shell, door, sack

PALETTE = BASE | {
    "wood": (0.34, 0.215, 0.115, 1), "deck": (0.39, 0.28, 0.16, 1),
    "edge": (0.23, 0.125, 0.055, 1), "oak": (0.13, 0.073, 0.038, 1),
    "wet": (0.074, 0.071, 0.046, 1), "waterline": (0.10, 0.145, 0.085, 1),
    "roof": (0.10, 0.225, 0.245, 1), "roof_dark": (0.058, 0.11, 0.12, 1),
    "ridge": (0.21, 0.29, 0.24, 1), "paint": (0.062, 0.20, 0.18, 1),
    "plaster": (0.49, 0.405, 0.275, 1), "stone": (0.33, 0.34, 0.305, 1),
    "dress": (0.48, 0.455, 0.365, 1), "cloth": (0.66, 0.53, 0.32, 1),
    "cloth_stripe": (0.34, 0.18, 0.095, 1), "rope": (0.40, 0.31, 0.165, 1),
}
DECK = 1.0
WALK_RECTS = [(-8.4, 6.4, -5.4, 2.0), (-2.2, 2.2, 2.0, 16.0), (-8.0, 8.0, 16.0, 20.0)]
ANCHORS = {
    "Anchor_Shore": (0, 0, DECK), "Anchor_PierEnd": (0, 20, DECK),
    "Anchor_Berth": (0, 22.6, 0), "Anchor_Departure": (15, 22.6, 0),
    "Anchor_GangwayPier": (0, 20, DECK), "Anchor_GangwayShip": (0, 21.05, 0.64),
    "Anchor_Door": (-5.25, 1.55, DECK), "Anchor_Cargo": (4.3, -1.4, DECK),
    "Light_Office": (-5.25, -2.0, 2.8), "Light_Door": (-6.38, 0.60, 3.38),
    "Light_Pier": (-7.1, 18.95, 3.70), "Anchor_HoistHook": (9.15, 18.4, 2.25),
}


def rope(mesh, points, radius=0.026):
    for a, b in zip(points, points[1:]):
        mesh.beam(a, b, radius * 2, radius * 2, "rope")


def rope_coil(mesh, x, y, z, radius=0.30):
    for turn in range(2):
        r = radius - turn * 0.065
        pts = [(x + r * math.cos(i * math.tau / 14), y + r * math.sin(i * math.tau / 14), z + 0.025) for i in range(15)]
        rope(mesh, pts, 0.025)


def crate(mesh, x, y, base, width=0.8, height=0.7):
    mesh.box((x, y, base + height / 2), (width, width * .84, height), "wood", .08)
    for z in (base + 0.06, base + height - .06):
        mesh.box((x, y, z), (width + .045, width * .84 + .045, .11), "edge")
    for sign in (-1, 1):
        mesh.beam((x-width*.40,y+sign*(width*.42+.025),base+.12),
                  (x+width*.40,y+sign*(width*.42+.025),base+height-.12),.09,.04,"edge")


def barrel(mesh, x, y, base, radius=.36, height=.95):
    profile = [(0,radius*.82),(.14*height,radius),(.70*height,radius),(height,radius*.82)]
    lathe(mesh,(x,y,base),profile,"wood",8)
    for z in (.16*height,.72*height):
        lathe(mesh,(x,y,base),[(z-.035,radius*1.015),(z+.035,radius*1.015)],"iron",8)
    mesh.box((x,y,base+height+.012),(radius*1.35,.035,.024),"edge")


def decking(mesh, x0, x1, y0, y1):
    """Solid backing closes the narrow board seams; every plank top is level."""
    mesh.box(((x0+x1)/2,(y0+y1)/2,DECK-.088),(x1-x0,y1-y0,.164),"oak")
    count = math.ceil((y1-y0)/.36)
    pitch = (y1-y0)/count
    for i in range(count):
        y = y0+(i+.5)*pitch
        # Break the long T-head spans over the underlying stringers.
        n = max(1, math.ceil((x1-x0)/4.4))
        for j in range(n):
            w=(x1-x0)/n
            mesh.box((x0+(j+.5)*w,y,DECK-.055),(w-.008,pitch-.009,.11),"deck",.12)


def pile(mesh, x, y, top=.97):
    # Fixed study depth: when integrated, underwater piles must follow the surveyed bed.
    mesh.box((x,y,-1.4),(.34,.34,2.8),"wet")
    mesh.box((x,y,.055),(.345,.345,.13),"waterline")
    mesh.box((x,y,(top+.07)/2),(.34,.34,top-.07),"oak")
    mesh.box((x,y,top-.055),(.38,.38,.07),"iron")


def bollard(mesh, x, y):
    mesh.box((x,y,DECK+.08),(.45,.45,.16),"oak")
    lathe(mesh,(x,y,DECK+.13),[(0,.13),(.34,.13),(.37,.19),(.44,.19)],"iron",8)
    mesh.box((x,y,DECK+.40),(.52,.105,.11),"iron")


def rails(mesh, x, y0, y1):
    ys=[y0,y1]
    for y in ys:
        mesh.box((x,y,DECK+.50),(.14,.14,1.15),"edge")
        mesh.box((x,y,DECK+1.11),(.20,.20,.08),"dress")
    for z in (DECK+.46,DECK+1.02):
        mesh.beam((x,y0,z),(x,y1,z),.085,.12,"edge")


def pier(deck, supports, details):
    decking(deck,-2.2,2.2,2,16)
    decking(deck,-8,8,16,20)
    for x in (-1.80,1.80):
        supports.box((x,9,.63),(.24,14.25,.24),"oak")
    ys=[2.15,5.6,9.1,12.55,15.8]
    for y in ys:
        for x in (-1.80,1.80): pile(supports,x,y)
        supports.box((0,y,.53),(4.16,.24,.32),"edge")
        for sign in (-1,1):
            supports.beam((sign*1.80,y,-.38),(sign*.7,y,.50),.16,.16,"edge")
    for x in (-7.55,-3.80,0,3.80,7.55):
        supports.box((x,18,.57),(.24,4.20,.26),"oak")
        for y in (16.4,19.55): pile(supports,x,y)
    for y in (16.4,19.55):
        supports.box((0,y,.42),(15.6,.24,.30),"edge")
        for a,b in zip((-7.55,-3.8,0,3.8),(-3.8,0,3.8,7.55)):
            supports.beam((a,y,-.9),(b,y,.40),.16,.16,"oak")
    # Fascias meet decks, while periodic vertical fenders take vessel contact.
    for x in (-2.18,2.18):
        supports.box((x,9,.83),(.14,14,.24),"edge")
    for y in (16.04,19.96):
        if y<17:
            for a,b in ((-8,-2.2),(2.2,8)):
                supports.box(((a+b)/2,y,.83),(b-a,.15,.24),"edge")
        else:
            supports.box((0,y,.83),(16,.15,.24),"edge")
    for x in (-7.97,7.97): supports.box((x,18,.83),(.15,4,.24),"edge")
    for x in (-6,-3,3,6):
        supports.box((x,20.08,.12),(.25,.22,1.8),"wet")
        details.box((x,20.10,.82),(.30,.26,.075),"iron")
        bollard(details,x,19.65)
    for x in (-1.98,1.98): rails(details,x,2.2,5.6)
    # Berth face and centre boarding gap intentionally have no fence.
    for x in (-7.65,7.65): bollard(details,x,17.0)
    rope_coil(details,-6.6,19.2,DECK)
    rope_coil(details,6.5,17.0,DECK,.27)


def wharf(mesh):
    # Stone shore landing: only the sea-facing retaining wall is exposed in the study.
    mesh.box((-1,-1.7,.15),(14.8,7.4,1.68),"mortar")
    for y in (-5.4,2.0):
        w=Wall(mesh,(0,y,0),(1,0,0),(0,1,0)) if y>0 else Wall(mesh,(0,y,0),(-1,0,0),(0,-1,0))
        a,b=(-8.4,6.4) if y>0 else (-6.4,8.4)
        masonry(w,a,b,-.68,.96,block_width=1.18,course=.48)
    for x in (-8.4,6.4):
        w=Wall(mesh,(x,0,0),(0,1,0),(-1,0,0)) if x<0 else Wall(mesh,(x,0,0),(0,-1,0),(1,0,0))
        a,b=(-5.4,2) if x<0 else (-2,5.4)
        masonry(w,a,b,-.68,.96,block_width=1.18,course=.48)
    # Broad flush flags, no raised lip across the shore entrance or pier.
    for row in range(7):
        for col in range(12):
            mesh.box((-8.4+(col+.5)*14.8/12,-5.4+(row+.5)*7.4/7,.972),
                     (14.8/12-.012,7.4/7-.012,.056),"stone",.065)


def office(body, leaf, glass):
    walls=shell(body,4.8,2.4,-2.4,3.05,5.20,door=(.77,2.28))
    for i,w in enumerate(walls):
        for a,b in segments(-2.4,2.4,[(-.77,.77)] if i==0 else []):
            masonry(w,a,b,-.12,.43,block_width=.84,course=.29)
    shingle_roof(body,2.85,2.87,-2.85,3.02,5.20,tile=.68)
    for w in (walls[0],walls[2]):
        w.box(0,.065,3.18,4.66,.19,.17,"oak")
        # Gable window occupies its own opening between diagonal framing.
        window(w,glass,0,3.5,.68,.93)
        for sign in (-1,1): w.beam(sign*2.16,3.22,sign*.52,4.64,.025,.12,.14,"edge")
    for u in (-1.52,1.52):
        window(walls[0],glass,u,1.15,.65,1.03)
        walls[0].box(u+(-.47 if u<0 else .47),.09,1.66,.19,.10,1.07,"paint")
    for w in walls[1:]:
        for u in (-1.1,1.1): window(w,glass,u,1.18,.90,1.14)
    pivot=door(walls[0],leaf,width=1.38,height=2.18)
    lantern(walls[0],glass,-1.13,2.38)
    # Timber rain hood, bearing on connected knee braces outside the door swing.
    outline=[(-1.02,2.45,2.88),(1.02,2.45,2.88),(1.02,3.16,2.67),(-1.02,3.16,2.67)]
    body.add(outline,[(3,2,1,0)],"roof")
    roof_underside(body,outline,"oak",.09)
    body.box((0,3.10,2.58),(2.12,.16,.16),"edge")
    for x in (-.98,.98):body.beam((x,2.46,2.15),(x,3.1,2.58),.085,.09,"edge")
    # Small projecting harbour sign: iron anchor pictogram, no baked text.
    w=walls[0]
    w.box(1.56,.43,2.82,.065,.91,.065,"iron")
    w.box(1.55,.83,2.82,.82,.065,.065,"iron")
    for u in (1.20,1.90):w.box(u,.83,2.61,.025,.025,.42,"iron")
    w.box(1.55,.83,2.31,.85,.09,.54,"paint")
    w.box(1.55,.885,2.32,.045,.025,.35,"dress")
    w.box(1.55,.885,2.39,.32,.025,.045,"dress")
    for sign in (-1,1):w.beam(1.55,2.15,1.55+sign*.23,2.26,.89,.04,.03,"dress")
    validate_swing(body,leaf,pivot)
    delta=(-5.25,-2.0,DECK)
    for m in (body,leaf,glass):shift(m,delta)
    return Vector(pivot)+Vector(delta)


def cargo_shelter(mesh, goods):
    # A dockside lean-to, open to the central aisle and visible from the sea.
    x0,x1,y0,y1=2.65,5.95,-3.85,.25
    for x in (x0,x1):
        for y in (y0,y1):
            top=3.90 if x==x0 else 3.45
            mesh.box((x,y,(DECK+top)/2),(.17,.17,top-DECK),"oak")
    for x,z in ((x0,3.85),(x1,3.40)):
        mesh.box((x,(y0+y1)/2,z),(.19,y1-y0+.25,.21),"edge")
        for y,sgn in ((y0,1),(y1,-1)):
            mesh.beam((x,y,z-.7),(x,y+sgn*.58,z),.10,.10,"edge")
    for y in (y0,y1):mesh.beam((x0,y,3.91),(x1,y,3.46),.13,.18,"edge")
    for i in range(7):
        a=y0-.25+i*(y1-y0+.50)/7;b=a+(y1-y0+.50)/7+.003
        outline=[(x0-.18,a,4.02),(x1+.20,a,3.57),(x1+.20,b,3.57),(x0-.18,b,4.02)]
        mesh.add(outline,[(0,1,2,3)],"cloth" if i%2==0 else "cloth_stripe")
        roof_underside(mesh,outline,"cloth",.05)
    # Every stack derives its base from its supporting pallet or crate top.
    goods.box((4.38,-2.70,1.10),(2.12,1.45,.20),"oak")
    for i in range(5):goods.box((3.54+i*.42,-2.70,1.235),(.40,1.44,.07),"deck")
    crate(goods,3.96,-2.70,1.27,.85,.72)
    crate(goods,4.90,-2.70,1.27,.78,.62)
    crate(goods,3.96,-2.70,1.99,.58,.48)
    for x,y in ((4.1,-.35),(5.05,-.7)):barrel(goods,x,y,DECK)
    sack(goods,5.10,-1.45,base=DECK,radius=.27,height=.74)
    rope_coil(goods,3.27,-.3,DECK)


def hoist(mesh):
    # Triangulated hand-operated dock crane. The mast bears on the head stringer.
    x,y=7.55,18.4
    mesh.box((x,y,1.12),(.75,.72,.24),"oak")
    mesh.box((x,y,3.20),(.27,.29,4.25),"edge")
    for sign in (-1,1):mesh.beam((x,y+sign*.85,1.02),(x,y,2.45),.17,.17,"oak")
    mesh.beam((x-.30,y,5.27),(9.15,y,5.27),.23,.25,"edge")
    mesh.beam((x,y,3.90),(8.95,y,5.22),.17,.17,"oak")
    # Winch axle with cheeks and a real crank; the line goes over its pulley.
    mesh.beam((x-.22,y-.42,2.10),(x-.22,y+.42,2.10),.20,.20,"oak")
    mesh.beam((x-.22,y-.48,2.10),(x-.22,y-.48,2.55),.07,.07,"iron")
    mesh.beam((x-.22,y-.48,2.55),(x-.22,y-.68,2.55),.075,.075,"edge")
    rope(mesh,[(x-.22,y,2.10),(x-.22,y,5.40),(9.15,y,5.40),(9.15,y,2.40)],.029)
    lathe(mesh,(9.15,y,2.40),[(0,.12),(.25,.12)],"iron",6)
    # Angular iron hook, connected all the way to its block.
    pts=[(9.15,y,2.42),(9.15,y,2.18),(9.31,y,2.11),(9.42,y,2.24)]
    for a,b in zip(pts,pts[1:]):mesh.beam(a,b,.055,.055,"iron")
    # A compact lantern mast at the opposite end balances the crane silhouette.
    mesh.box((-7.1,18.95,2.43),(.16,.16,2.88),"edge")
    mesh.beam((-7.1,18.95,3.84),(-6.65,18.95,3.84),.08,.08,"iron")
    mesh.box((-6.65,18.95,3.53),(.24,.23,.40),"bronze")
    mesh.box((-6.65,18.95,3.79),(.33,.32,.10),"iron")
    mesh.box((-6.65,18.95,3.30),(.30,.29,.08),"iron")


def export_glb(path, objects, animations=True):
    bpy.ops.object.select_all(action="DESELECT")
    for obj in objects:obj.select_set(True)
    bpy.ops.export_scene.gltf(filepath=str(path),export_format="GLB",use_selection=True,
        export_yup=True,export_skins=False,export_texcoords=False,export_normals=True,
        export_tangents=False,export_cameras=False,export_lights=False,export_extras=True,
        export_animations=animations,export_animation_mode="ACTIONS",export_bake_animation=True,
        export_optimize_animation_size=False)


def stats(path):
    raw=path.read_bytes();size=struct.unpack_from('<I',raw,12)[0];doc=json.loads(raw[20:20+size])
    ps=[p for m in doc['meshes'] for p in m['primitives']]
    return {'gpu_vertices':sum(doc['accessors'][p['attributes']['POSITION']]['count'] for p in ps),
        'triangles':sum(doc['accessors'][p['indices']]['count']//3 for p in ps),
        'bytes':len(raw),'meshes':len(doc['meshes']),'materials':len(doc['materials']),
        'animations':[a['name'] for a in doc.get('animations',[])]}


def build():
    reset();scene=bpy.context.scene;scene.name="Harbour draft — not integrated"
    scene.render.fps=24;scene.frame_start=0;scene.frame_end=120
    meshes={n:BuildingMesh(PALETTE,200+i) for i,n in enumerate((
        'PortWharf','PortPierDeck','PortPierSupports','PortMoorings','PortOffice','PortOfficeDoor',
        'PortGlass','PortCargoShelter','PortCargo','PortHoist'))}
    wharf(meshes['PortWharf']);pier(meshes['PortPierDeck'],meshes['PortPierSupports'],meshes['PortMoorings'])
    pivot=office(meshes['PortOffice'],meshes['PortOfficeDoor'],meshes['PortGlass'])
    cargo_shelter(meshes['PortCargoShelter'],meshes['PortCargo']);hoist(meshes['PortHoist'])
    mat=palette_material('PortPalette');glassmat=palette_material('PortGlass')
    objects=[]
    for name,mesh in meshes.items():
        obj=mesh.object(name,glassmat if name=='PortGlass' else mat,pivot if name=='PortOfficeDoor' else (0,0,0))
        if name=='PortOfficeDoor':animate_door(obj)
        objects.append(obj)
    for name,point in ANCHORS.items():
        obj=bpy.data.objects.new(name,None);scene.collection.objects.link(obj);obj.location=point
        obj.empty_display_type='PLAIN_AXES';obj.empty_display_size=.25;objects.append(obj)
    export_glb(HERE/'PortDraft.glb',objects)
    counts=stats(HERE/'PortDraft.glb');counts['editable_vertices']=sum(len(o.data.vertices) for o in objects if o.type=='MESH')
    assert counts['gpu_vertices']<24000,counts
    (HERE/'port-contract.json').write_text(json.dumps({
        'status':'standalone art draft; not registered in the game',
        'axes':'Blender +Y seaward / glTF -Z seaward; origin at shore water datum',
        'deck_height':DECK,'main_pier_width':4.4,'main_pier_length':14.0,
        'head_width':16.0,'head_depth':4.0,'shore_to_head':20.0,
        'walkable_rectangles_blender_xy':WALK_RECTS,'minimum_clear_main_lane':2.4,
        'primary_cog_envelope_blender':{'center':[0,22.6,0],'length_along_x':9.0,'beam':3.2,'draft':1.0},
        'anchors_blender':ANCHORS,'geometry':counts,
        'integration_notes':['The existing port uses a 2 m surveyed straight pier, one berth and variable length.',
            'This wider T-head requires a later survey/navigation contract update; do not replace procedural geometry blindly.',
            'Pile bottoms are a -2.8 m study depth; adapt them to the surveyed seabed.',
            'Never bake one solid convex hull around the port. Walkable deck and obstacle data must remain separate.',
            'Door clips and light anchors are prepared; runtime wiring is intentionally not added.']},indent=2)+'\n')
    # Navigation authoring surfaces: separate file, excluded from the beauty asset.
    nav=BuildingMesh(PALETTE)
    for x0,x1,y0,y1 in WALK_RECTS:nav.add([(x0,y0,DECK),(x1,y0,DECK),(x1,y1,DECK),(x0,y1,DECK)],[(0,1,2,3)],'paint')
    proxy=nav.object('Navigation_DeckSurfaces',mat)
    export_glb(HERE/'PortWalkSurfaces.glb',[proxy],False)
    proxy.hide_render=True;proxy.hide_set(True);proxy.display_type='WIRE'
    proxy['Purpose']='Draft walkable surfaces. Subtract office/cargo obstacles on integration.'
    for o in objects:
        if o.type=='EMPTY':o.hide_set(True)
    bpy.ops.object.camera_add(location=(32,38,29));camera=bpy.context.object;camera.name='StudioCamera'
    aim=Vector((-.5,7.5,1.5));camera.rotation_euler=(aim-camera.location).to_track_quat('-Z','Y').to_euler()
    camera.data.type='ORTHO';camera.data.ortho_scale=37;scene.camera=camera
    for location,power,size in [((4,8,25),2600,12),((-14,5,15),1500,10)]:
        bpy.ops.object.light_add(type='AREA',location=location);light=bpy.context.object;light.data.energy=power;light.data.size=size
        light.rotation_euler=(aim-light.location).to_track_quat('-Z','Y').to_euler()
    scene.render.engine='CYCLES';scene.cycles.samples=32
    scene.render.resolution_x=1600;scene.render.resolution_y=1200;scene.render.resolution_percentage=100
    scene.world.color=(.22,.22,.22);scene.view_settings.view_transform='AgX'
    scene['Status']='Standalone harbour asset study. No game integration.'
    for screen in bpy.data.screens:
        for area in screen.areas:
            if area.type=='VIEW_3D':
                s=area.spaces.active;s.shading.type='MATERIAL';s.shading.color_type='VERTEX'
                s.shading.show_backface_culling=True;s.overlay.show_extras=False;s.overlay.show_stats=True
                s.region_3d.view_location=aim;s.region_3d.view_distance=38
                s.region_3d.view_rotation=camera.rotation_euler.to_quaternion();s.region_3d.view_perspective='PERSP'
    bpy.ops.object.select_all(action='DESELECT');scene.frame_set(0)
    bpy.context.preferences.filepaths.save_version=0
    bpy.ops.wm.save_as_mainfile(filepath=str(HERE/'port.blend'))
    print('PORT_DRAFT '+json.dumps(counts),flush=True)


if __name__=='__main__':build()
