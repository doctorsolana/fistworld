"""Baseline soldier sidearm; grip at origin, blade along +Z like existing tools.
Run with Blender --background --factory-startup --python this_file.py.
"""
import os
import bpy
bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete(use_global=False)
materials=[]
for name,color in [('Steel',(0.48,0.54,0.60,1)),('Edge',(0.78,0.82,0.84,1)),('Leather',(0.12,0.065,0.035,1)),('Guard',(0.30,0.23,0.10,1))]:
    mat=bpy.data.materials.new(name);mat.diffuse_color=color;mat.use_nodes=True
    bsdf=mat.node_tree.nodes.get('Principled BSDF');bsdf.inputs['Base Color'].default_value=color;bsdf.inputs['Roughness'].default_value=.85
    materials.append(mat)
parts=[]
def box(name,loc,scale,mat):
    bpy.ops.mesh.primitive_cube_add(size=1,location=loc)
    ob=bpy.context.object;ob.name=name;ob.scale=scale
    bpy.ops.object.transform_apply(location=False,rotation=False,scale=True)
    ob.data.materials.append(materials[mat]);parts.append(ob)
box('Grip',(0,0,-.015),(.033,.035,.15),2)
box('Pommel',(0,0,-.105),(.055,.04,.045),3)
box('Guard',(0,0,.075),(.20,.045,.033),3)
verts=[(-.035,0,.095),(0,-.012,.095),(.035,0,.095),(0,.012,.095),(-.027,0,.60),(0,-.010,.60),(.027,0,.60),(0,.010,.60),(0,0,.76)]
faces=[(0,1,5,4),(1,2,6,5),(2,3,7,6),(3,0,4,7),(4,5,8),(5,6,8),(6,7,8),(7,4,8),(3,2,1,0)]
me=bpy.data.meshes.new('Blade');me.from_pydata(verts,[],faces);me.materials.append(materials[0]);me.materials.append(materials[1])
for p in me.polygons:p.material_index=p.index%2
ob=bpy.data.objects.new('Blade',me);bpy.context.collection.objects.link(ob);parts.append(ob)
bpy.ops.object.select_all(action='DESELECT')
for ob in parts:ob.select_set(True)
bpy.context.view_layer.objects.active=parts[0];bpy.ops.object.join();ob=bpy.context.object;ob.name='SoldierSidearm'
bpy.context.scene.cursor.location=(0,0,0);bpy.ops.object.origin_set(type='ORIGIN_CURSOR')
# Match export_resources_glb.py's grip basis: source +Z becomes glTF +Y.
out=os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))),'client/assets/game_assets/tools/SoldierSidearm.glb')
bpy.ops.export_scene.gltf(filepath=out,export_format='GLB',use_selection=True,export_yup=True,export_animations=False,export_materials='EXPORT')
print('Sidearm vertices:',len(ob.data.vertices),'triangles:',sum(len(p.vertices)-2 for p in ob.data.polygons))
