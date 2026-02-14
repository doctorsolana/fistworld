#!/bin/bash
# Convert custom NPC models to use KayKit bone naming convention
#
# This script uses Blender in headless mode to rename bones and re-export models.
#
# Usage: ./convert_models_to_kaykit.sh
#
# Requirements:
# - Blender must be installed and accessible via command line
# - On macOS: /Applications/Blender.app/Contents/MacOS/Blender
# - On Linux: blender

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
ASSETS_DIR="$PROJECT_DIR/client/assets/misc"
PYTHON_SCRIPT="$SCRIPT_DIR/rename_bones_to_kaykit.py"

# Find Blender
if [[ "$OSTYPE" == "darwin"* ]]; then
    BLENDER="/Applications/Blender.app/Contents/MacOS/Blender"
else
    BLENDER="blender"
fi

if ! command -v "$BLENDER" &> /dev/null && [[ ! -x "$BLENDER" ]]; then
    echo "ERROR: Blender not found at $BLENDER"
    echo "Please install Blender or update the BLENDER path in this script."
    exit 1
fi

echo "=============================================="
echo "KayKit Bone Converter"
echo "=============================================="
echo "Blender: $BLENDER"
echo "Script: $PYTHON_SCRIPT"
echo "Assets: $ASSETS_DIR"
echo ""

# Create a temporary Python script that processes a single file
create_processor_script() {
    local input_file="$1"
    local output_file="$2"

    cat << 'PYTHON_EOF'
import bpy
import sys
import os

# Clear the scene
bpy.ops.wm.read_factory_settings(use_empty=True)

# Get command line args after --
argv = sys.argv
if "--" in argv:
    argv = argv[argv.index("--") + 1:]
else:
    argv = []

if len(argv) < 2:
    print("Usage: blender --background --python script.py -- input.glb output.glb")
    sys.exit(1)

input_file = argv[0]
output_file = argv[1]

print(f"\n=== Processing: {input_file} ===\n")

# Import the GLB
bpy.ops.import_scene.gltf(filepath=input_file)

# Bone mapping
BONE_MAPPING = {
    "Root": "root",
    "Pelvis": "hips",
    "Spine_01": "spine",
    "Spine_02": "chest",
    "Spine_03": "chest.001",
    "Neck_01": "neck",
    "Head": "head",
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
    "Thigh_L": "upperleg.l",
    "Calf_L": "lowerleg.l",
    "Foot_L": "foot.l",
    "Ball_L": "toes.l",
    "Thigh_R": "upperleg.r",
    "Calf_R": "lowerleg.r",
    "Foot_R": "foot.r",
    "Ball_R": "toes.r",
}

# Find armature
armatures = [o for o in bpy.data.objects if o.type == 'ARMATURE']
if not armatures:
    print("ERROR: No armature found!")
    sys.exit(1)

for armature_obj in armatures:
    print(f"Processing armature: {armature_obj.name}")

    # Rename armature to Rig_Medium
    if "Armature" in armature_obj.name:
        armature_obj.name = "Rig_Medium"
        armature_obj.data.name = "Rig_Medium"

    # Enter edit mode
    bpy.context.view_layer.objects.active = armature_obj
    bpy.ops.object.mode_set(mode='EDIT')

    armature = armature_obj.data
    bone_names = [bone.name for bone in armature.edit_bones]

    renamed = 0
    for old_name in bone_names:
        if old_name in BONE_MAPPING:
            new_name = BONE_MAPPING[old_name]
            bone = armature.edit_bones.get(old_name)
            if bone:
                bone.name = new_name
                print(f"  {old_name} -> {new_name}")
                renamed += 1

    bpy.ops.object.mode_set(mode='OBJECT')
    print(f"  Renamed {renamed} bones")

# Export
print(f"\nExporting to: {output_file}")
bpy.ops.export_scene.gltf(
    filepath=output_file,
    export_format='GLB',
    export_animations=True,
    export_skins=True,
)

print("Done!")
PYTHON_EOF
}

# Process each model
process_model() {
    local input_file="$1"
    local filename=$(basename "$input_file")
    local backup_file="${input_file%.glb}_original.glb"

    if [[ ! -f "$input_file" ]]; then
        echo "WARNING: File not found: $input_file"
        return 1
    fi

    echo ""
    echo "Processing: $filename"
    echo "----------------------------------------------"

    # Create backup if it doesn't exist
    if [[ ! -f "$backup_file" ]]; then
        echo "Creating backup: $backup_file"
        cp "$input_file" "$backup_file"
    fi

    # Create temp script
    local temp_script=$(mktemp /tmp/blender_convert_XXXXXX.py)
    create_processor_script > "$temp_script"

    # Run Blender in background mode
    "$BLENDER" --background --python "$temp_script" -- "$input_file" "$input_file"

    # Clean up
    rm -f "$temp_script"

    echo "Done: $filename"
}

# Process the models
process_model "$ASSETS_DIR/king.glb"
process_model "$ASSETS_DIR/peanasnt.glb"

echo ""
echo "=============================================="
echo "All models processed!"
echo "=============================================="
echo ""
echo "Original files backed up as *_original.glb"
echo "You can now run the game to test animations."
