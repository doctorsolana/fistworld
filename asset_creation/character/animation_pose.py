"""Pose isolation shared by the character exporter, validators and previews.

An action only owns the channels it keys. Reset before binding, otherwise a
previous death/swim pose can contaminate an unrelated work clip during baking.
"""

from mathutils import Matrix


def bind_action(rig, action, slot=None):
    rig.animation_data_create()
    rig.animation_data.action = None
    for track in rig.animation_data.nla_tracks:
        track.mute = True
    for bone in rig.pose.bones:
        bone.matrix_basis = Matrix.Identity(4)
    rig.animation_data.action = action
    if slot is not None:
        rig.animation_data.action_slot = slot
    elif action.slots:
        rig.animation_data.action_slot = action.slots[0]
