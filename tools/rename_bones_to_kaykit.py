"""
Blender Python Script: Rename Custom Model Bones to KayKit Convention

This script renames bones in custom character models (King, Peasant) to match
the KayKit Adventurers bone naming convention so they can use KayKit animations.

Usage in Blender:
1. Open the .glb file in Blender
2. Select the Armature object
3. Run this script (Text Editor > Run Script or Alt+P)
4. Export as .glb (File > Export > glTF 2.0)

The script handles the naming differences:
- Custom: PascalCase with _L/_R suffixes (e.g., "Upperarm_L")
- KayKit: lowercase with .l/.r suffixes (e.g., "upperarm.l")
"""

import bpy

# Bone name mapping: Custom -> KayKit
BONE_MAPPING = {
    # Root and spine
    "Root": "root",
    "Pelvis": "hips",
    "Spine_01": "spine",
    "Spine_02": "chest",
    "Spine_03": "chest.001",  # Upper chest (KayKit may not have this, but keep it)

    # Head and neck
    "Neck_01": "neck",
    "Head": "head",

    # Left arm
    "Clavicle_L": "clavicle.l",
    "Upperarm_L": "upperarm.l",
    "Lowerarm_L": "lowerarm.l",
    "Hand_L": "hand.l",
    "Thumb_01_L": "thumb.01.l",
    "Thumb_02_L": "thumb.02.l",
    "Thumb_03_L": "thumb.03.l",
    "Index_01_L": "index.01.l",
    "Index_02_L": "index.02.l",
    "Index_03_L": "index.03.l",
    "Middle_01_L": "middle.01.l",
    "Middle_02_L": "middle.02.l",
    "Middle_03_L": "middle.03.l",
    "Ring_01_L": "ring.01.l",
    "Ring_02_L": "ring.02.l",
    "Ring_03_L": "ring.03.l",
    "Pinky_01_L": "pinky.01.l",
    "Pinky_02_L": "pinky.02.l",
    "Pinky_03_L": "pinky.03.l",

    # Right arm
    "Clavicle_R": "clavicle.r",
    "Upperarm_R": "upperarm.r",
    "Lowerarm_R": "lowerarm.r",
    "Hand_R": "hand.r",
    "Thumb_01_R": "thumb.01.r",
    "Thumb_02_R": "thumb.02.r",
    "Thumb_03_R": "thumb.03.r",
    "Index_01_R": "index.01.r",
    "Index_02_R": "index.02.r",
    "Index_03_R": "index.03.r",
    "Middle_01_R": "middle.01.r",
    "Middle_02_R": "middle.02.r",
    "Middle_03_R": "middle.03.r",
    "Ring_01_R": "ring.01.r",
    "Ring_02_R": "ring.02.r",
    "Ring_03_R": "ring.03.r",
    "Pinky_01_R": "pinky.01.r",
    "Pinky_02_R": "pinky.02.r",
    "Pinky_03_R": "pinky.03.r",

    # Left leg
    "Thigh_L": "upperleg.l",
    "Calf_L": "lowerleg.l",
    "Foot_L": "foot.l",
    "Ball_L": "toes.l",

    # Right leg
    "Thigh_R": "upperleg.r",
    "Calf_R": "lowerleg.r",
    "Foot_R": "foot.r",
    "Ball_R": "toes.r",
}

# Armature name mapping
ARMATURE_MAPPING = {
    "Armature": "Rig_Medium",
    "Armature.001": "Rig_Medium",
    "Armature.002": "Rig_Medium",
}


def rename_bones():
    """Rename bones in the selected armature to match KayKit convention."""

    # Get the active object (should be an armature)
    obj = bpy.context.active_object

    if obj is None:
        print("ERROR: No object selected. Please select the Armature.")
        return False

    if obj.type != 'ARMATURE':
        # Try to find an armature in the scene
        armatures = [o for o in bpy.data.objects if o.type == 'ARMATURE']
        if armatures:
            obj = armatures[0]
            print(f"Found armature: {obj.name}")
        else:
            print("ERROR: No armature found in the scene.")
            return False

    armature = obj.data

    print(f"\n=== Renaming bones in armature: {obj.name} ===\n")

    # First, rename the armature object itself if needed
    if obj.name in ARMATURE_MAPPING:
        new_name = ARMATURE_MAPPING[obj.name]
        print(f"Renaming armature object: {obj.name} -> {new_name}")
        obj.name = new_name
        armature.name = new_name

    # Switch to edit mode to rename bones
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.mode_set(mode='EDIT')

    renamed_count = 0
    skipped_count = 0

    # Get list of bone names first (can't modify while iterating)
    bone_names = [bone.name for bone in armature.edit_bones]

    for old_name in bone_names:
        if old_name in BONE_MAPPING:
            new_name = BONE_MAPPING[old_name]
            bone = armature.edit_bones.get(old_name)
            if bone:
                bone.name = new_name
                print(f"  Renamed: {old_name} -> {new_name}")
                renamed_count += 1
        else:
            print(f"  Skipped (no mapping): {old_name}")
            skipped_count += 1

    # Return to object mode
    bpy.ops.object.mode_set(mode='OBJECT')

    print(f"\n=== Done! ===")
    print(f"Renamed: {renamed_count} bones")
    print(f"Skipped: {skipped_count} bones")
    print(f"\nNext steps:")
    print(f"1. File > Export > glTF 2.0 (.glb)")
    print(f"2. Overwrite the original file or save with new name")

    return True


def list_bones():
    """List all bones in the selected armature (for debugging)."""
    obj = bpy.context.active_object

    if obj is None or obj.type != 'ARMATURE':
        armatures = [o for o in bpy.data.objects if o.type == 'ARMATURE']
        if armatures:
            obj = armatures[0]
        else:
            print("No armature found")
            return

    print(f"\n=== Bones in {obj.name} ===")
    for bone in obj.data.bones:
        parent_name = bone.parent.name if bone.parent else "(root)"
        print(f"  {bone.name} <- {parent_name}")


# Run the script
if __name__ == "__main__":
    print("\n" + "="*50)
    print("KayKit Bone Renamer")
    print("="*50)

    # Uncomment to list bones first:
    # list_bones()

    rename_bones()
