"""Report whether the Tripo entrance is a separable connected mesh component.

    blender asset_creation/tripoexports/tavern_cleaned.blend --background --python \
        asset_creation/tripoexports/analyse_tavern_door.py

This is diagnostic only; it never saves the blend.  The candidate volume is the visible door leaf on
the canonical +Y facade, not the porch posts or the Anchor_Door stand point outside it.
"""

from collections import defaultdict, deque

import bpy


shell = bpy.data.objects["TavernShell"]
mesh = shell.data

vert_faces: dict[int, list[int]] = defaultdict(list)
for poly in mesh.polygons:
    for vertex in poly.vertices:
        vert_faces[vertex].append(poly.index)

unseen = set(range(len(mesh.polygons)))
components: list[list[int]] = []
while unseen:
    seed = unseen.pop()
    queue = deque([seed])
    component = [seed]
    while queue:
        face_index = queue.popleft()
        for vertex in mesh.polygons[face_index].vertices:
            for neighbour in vert_faces[vertex]:
                if neighbour in unseen:
                    unseen.remove(neighbour)
                    queue.append(neighbour)
                    component.append(neighbour)
    components.append(component)

# Measured from the corrected front render.  A component is a candidate if its world-space bounding
# box intersects this compact volume; components spanning far outside it are frame/wall pieces.
door_lo = (+0.18, 3.45, 0.55)
door_hi = (+1.62, 5.02, 3.05)
candidates = []
for faces in components:
    vertices = {vertex for face in faces for vertex in mesh.polygons[face].vertices}
    coords = [mesh.vertices[index].co for index in vertices]
    lo = tuple(min(co[axis] for co in coords) for axis in range(3))
    hi = tuple(max(co[axis] for co in coords) for axis in range(3))
    intersects = all(hi[axis] >= door_lo[axis] and lo[axis] <= door_hi[axis] for axis in range(3))
    if intersects:
        candidates.append((len(faces), len(vertices), lo, hi))

candidates.sort(reverse=True)
print(f"[door] shell: {len(mesh.vertices)} vertices, {len(mesh.polygons)} polygons")
print(f"[door] connected components: {len(components)}")
print(f"[door] components intersecting candidate volume: {len(candidates)}")
for faces, vertices, lo, hi in candidates[:24]:
    print(
        f"[door] {faces:4d} faces {vertices:4d} verts  "
        f"x {lo[0]:+.3f}..{hi[0]:+.3f}  y {lo[1]:+.3f}..{hi[1]:+.3f}  "
        f"z {lo[2]:+.3f}..{hi[2]:+.3f}"
    )
