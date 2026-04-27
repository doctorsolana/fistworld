//! NPC identity helpers.

use shared::components::{NpcArchetype, NpcIdentity};
use shared::map::{MapBehaviorPreset, MapNpcGroup};
use shared::npc::npc_name_for_id;

pub(crate) fn npc_identity_for_group(seed: u32, npc_id: u64, group: &MapNpcGroup) -> NpcIdentity {
    NpcIdentity {
        name: npc_name_for_id(seed, npc_id),
        occupation: group
            .authored_occupation()
            .unwrap_or(default_occupation_for_preset(group.preset))
            .to_string(),
        faction: group.authored_faction().map(str::to_string),
    }
}

pub(crate) fn npc_identity_for_archetype(
    seed: u32,
    npc_id: u64,
    archetype: NpcArchetype,
) -> NpcIdentity {
    NpcIdentity {
        name: npc_name_for_id(seed, npc_id),
        occupation: default_occupation_for_archetype(archetype).to_string(),
        faction: None,
    }
}

pub(crate) fn npc_identity_for_debug(
    seed: u32,
    npc_id: u64,
    archetype: NpcArchetype,
) -> NpcIdentity {
    NpcIdentity {
        occupation: format!("Debug {}", default_occupation_for_archetype(archetype)),
        ..npc_identity_for_archetype(seed, npc_id, archetype)
    }
}

fn default_occupation_for_preset(preset: MapBehaviorPreset) -> &'static str {
    match preset {
        MapBehaviorPreset::IdleWanderZone => "Pedestrian",
        MapBehaviorPreset::PatrolRoute => "Patrol",
        MapBehaviorPreset::StandAndFaceFlow => "Vendor",
    }
}

fn default_occupation_for_archetype(archetype: NpcArchetype) -> &'static str {
    match archetype {
        NpcArchetype::Oilman => "Pedestrian",
        NpcArchetype::DesertOutpost => "Guard",
    }
}
