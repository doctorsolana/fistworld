"""Rebuild the metre-scale, articulated catapult. Blender --background --python this_file.
Authored coordinates below are Bevy (+Y up, -Z firing direction). No external assets.
"""
import bpy, math, json
from pathlib import Path
from mathutils import Vector
ROOT=Path(__file__).resolve().parents[2]
bpy.ops.object.select_all(action='SELECT'); bpy.ops.object.delete(use_global=False)
def material(name,color,metal=0):
    m=bpy.data.materials.new(name);m.diffuse_color=(*color,1);m.use_nodes=True
    p=m.node_tree.nodes.get('Principled BSDF');p.inputs['Base Color'].default_value=(*color,1);p.inputs['Roughness'].default_value=.8;p.inputs['Metallic'].default_value=metal
    return m
MATS=[material('Honey oak',(.39,.21,.085)),material('Cut oak',(.62,.37,.16)),material('Dark endgrain',(.23,.105,.04)),material('Forged iron',(.095,.105,.11),.65),material('Hemp rope',(.60,.49,.29)),material('Leather sling',(.20,.09,.038)),material('Brass fittings',(.65,.43,.13),.55)]
def xyz(v):return (v[0],-v[2],v[1])
def empty(name,p=(0,0,0)):
    o=bpy.data.objects.new(name,None);bpy.context.collection.objects.link(o);o.location=xyz(p);return o
class Mesh:
    def __init__(self):self.v=[];self.f=[];self.m=[]
    def poly(self,verts,faces,mat):
        n=len(self.v);self.v+=list(verts);self.f += [tuple(n+i for i in f) for f in faces];self.m += [mat]*len(faces)
    def box(self,p,size,mat=0):
        x,y,z=p;a,b,c=[v/2 for v in size]
        self.poly([(x+i*a,y+j*b,z+k*c) for i,j,k in [(-1,-1,-1),(1,-1,-1),(1,1,-1),(-1,1,-1),(-1,-1,1),(1,-1,1),(1,1,1),(-1,1,1)]],[(0,3,2,1),(4,5,6,7),(0,1,5,4),(1,2,6,5),(2,3,7,6),(3,0,4,7)],mat)
    def beam(self,a,b,width,depth=None,mat=0):
        a,b=Vector(a),Vector(b);d=(b-a).normalized();u=d.cross(Vector((0,1,0)))
        if u.length<.01:u=d.cross(Vector((1,0,0)))
        u.normalize();u*=width/2;v=d.cross(u).normalized()*(depth or width)/2
        self.poly([tuple(p+i*u+j*v) for p in (a,b) for i,j in [(-1,-1),(1,-1),(1,1),(-1,1)]],[(0,3,2,1),(4,5,6,7),(0,1,5,4),(1,2,6,5),(2,3,7,6),(3,0,4,7)],mat)
    def cylinder(self,a,b,r,mat=3,n=12):
        a,b=Vector(a),Vector(b);d=(b-a).normalized();u=d.cross(Vector((0,1,0)))
        if u.length<.01:u=d.cross(Vector((1,0,0)))
        u.normalize();v=d.cross(u)
        verts=[tuple(p+r*(math.cos(i*math.tau/n)*u+math.sin(i*math.tau/n)*v)) for p in (a,b) for i in range(n)]
        faces=[tuple(reversed(range(n))),tuple(range(n,2*n))]+[(i,(i+1)%n,(i+1)%n+n,i+n) for i in range(n)]
        self.poly(verts,faces,mat)
    def ring(self,outer,inner,width,mat=0,n=16):
        verts=[(x,r*math.cos(i*math.tau/n),r*math.sin(i*math.tau/n)) for x,r in [(-width/2,outer),(width/2,outer),(-width/2,inner),(width/2,inner)] for i in range(n)]
        faces=[]
        for i in range(n):
            j=(i+1)%n;faces.extend([(i,j,j+n,i+n),(i+2*n,i+3*n,j+3*n,j+2*n),(i,i+2*n,j+2*n,j),(i+n,j+n,j+3*n,i+3*n)])
        self.poly(verts,faces,mat)
    def emit(self,name,parent=None):
        mesh=bpy.data.meshes.new(name);mesh.from_pydata([xyz(v) for v in self.v],[],self.f);mesh.update()
        for m in MATS:mesh.materials.append(m)
        for f,m in zip(mesh.polygons,self.m):f.material_index=m
        obj=bpy.data.objects.new(name,mesh);bpy.context.collection.objects.link(obj);obj.parent=parent;return obj
root=empty('Catapult')
base=Mesh()
for x in [-.92,.92]:
    base.box((x,.69,0),(.26,.32,4.15))
    base.box((x,1.37,-.10),(.30,1.50,.30))
    base.beam((x,.83,1.25),(x,2.08,-.1),.19,mat=1)
    base.beam((x,.83,-1.55),(x,2.08,-.1),.18)
    for z in [-1.9,-.2,1.85]:base.box((x,.70,z),(.29,.35,.095),3)
    for y in [1.05,1.88]:base.box((x,y,-.1),(.33,.13,.34),3)
for z in [-1.5,.2,1.55]:
    base.box((0,.67,z),(2.05,.23,.24),1)
    if z!=.2:base.cylinder((-1.47,.61,z),(1.47,.61,z),.10)
for i in range(5):base.box((-.7+i*.35,.88,1.22),(.29,.10,1.36),i%2)
# Padded crossbar catches the arm at the end of its violent release.
base.box((0,2.15,-.45),(2.28,.28,.32),0)
base.box((0,2.18,-.25),(.85,.30,.20),5)
for x in [-.95,.95]:
    for z in [-1.50,1.55]:
        wheel=empty('Wheel'+str(x)+str(z),(x*1.41,.61,z));wheel.parent=root
        m=Mesh();m.ring(.60,.42,.20,0);m.ring(.616,.586,.215,3)
        m.cylinder((-.19,0,0),(.19,0,0),.14,1)
        for i in range(8):
            a=i*math.tau/8;m.beam((0,0,0),(0,.46*math.cos(a),.46*math.sin(a)),.085,mat=1)
        m.cylinder((-.22,0,0),(.22,0,0),.07,3,8);m.emit('Spoked wheel',wheel)
# Dense twisted skein, rendered as eight alternating strands around the axle.
base.cylinder((-1.16,1.75,0),(1.16,1.75,0),.19,4,12)
for i in range(12):
    x=-1.02+i*.18
    for j in range(4):
        a=j*math.pi/2+i*.44
        base.cylinder((x,1.75+.20*math.cos(a),.20*math.sin(a)),(x+.16,1.75+.20*math.cos(a+.44),.20*math.sin(a+.44)),.032,4,5)
base.emit('Timber carriage',root)
arm=empty('CatapultArm',(0,1.75,0));arm.parent=root
m=Mesh();m.beam((0,0,-.3),(0,0,2.10),.21,.25,1);m.box((0,0,.05),(.32,.32,.48),3)
for z in [.65,1.65]:m.box((0,0,z),(.245,.28,.09),3)
# Open wooden spoon with a leather-lined concavity. The socket sits inside it.
m.box((0,-.08,2.13),(.64,.11,.69),5)
for x in [-.31,.31]:m.box((x,.06,2.13),(.09,.30,.71),1)
for z in [1.81,2.46]:m.box((0,.06,z),(.64,.30,.09),1)
m.emit('Throwing arm',arm)
winch=empty('Winch',(0,1.11,1.73));winch.parent=root
m=Mesh();m.cylinder((-.9,0,0),(.9,0,0),.12,1)
for x in [-.84,.84]:
    m.beam((x,-.31,0),(x,.31,0),.09,mat=3)
    m.cylinder((x,.31,0),(x,.31,.22),.045,1,8)
for i in range(10):m.cylinder((-.25+i*.05,0,0),(-.21+i*.05,0,0),.148,4,10)
m.emit('Crank and ratchet',winch)
# Load-bearing chain is procedural in game so it follows the moving arm.
for o in bpy.context.scene.objects:o.select_set(True)
out=ROOT/'client/assets/game_assets/vehicles/Catapult.glb';out.parent.mkdir(parents=True,exist_ok=True)
bpy.ops.export_scene.gltf(filepath=str(out),export_format='GLB',export_yup=True,export_animations=False,export_cameras=False,export_lights=False)
meshes=[o.data for o in bpy.context.scene.objects if o.type=='MESH']
print(json.dumps({'vertices':sum(len(m.vertices) for m in meshes),'triangles':sum(len(p.vertices)-2 for m in meshes for p in m.polygons),'objects':len(meshes),'output':str(out)}))
