//! Bakery loaf presentation from replicated physical inventory.

use super::buildings::BuildingVisual;
use bevy::prelude::*;
use shared::components::{SettlementBuilding, SettlementBuildingKind};
use shared::economy::{Good, GoodsInventory};

/// Six authored loaf meshes presenting the bakery's real Bread inventory.
#[derive(Component)]
pub(super) struct BakeryBreadDisplay {
    pub(super) loaves: [Entity; BAKERY_BREAD_MESH_COUNT],
    pub(super) visible: u8,
}

pub(super) const BAKERY_BREAD_MESH_COUNT: usize = 6;

/// Discover the bakery's six separately authored loaf meshes once its scene
/// has instantiated. Their visibility is derived locally from the already
/// replicated inventory, so this visual truth costs no additional packets.
pub(super) fn setup_bakery_bread_display(
    mut commands: Commands,
    bakeries: Query<
        (Entity, &SettlementBuilding, &GoodsInventory),
        (With<BuildingVisual>, Without<BakeryBreadDisplay>),
    >,
    children: Query<&Children>,
    names: Query<&Name>,
    mut visibility: Query<&mut Visibility>,
) {
    for (bakery, building, inventory) in bakeries.iter() {
        if building.kind != SettlementBuildingKind::Bakery {
            continue;
        }

        let mut loaves = [Entity::PLACEHOLDER; BAKERY_BREAD_MESH_COUNT];
        let mut found = 0usize;
        let mut stack = vec![bakery];
        while let Some(entity) = stack.pop() {
            if let Ok(name) = names.get(entity) {
                if let Some(index) = name
                    .as_str()
                    .strip_prefix("Stock_Bread_")
                    .and_then(|suffix| suffix.parse::<usize>().ok())
                    .and_then(|number| number.checked_sub(1))
                    .filter(|index| *index < BAKERY_BREAD_MESH_COUNT)
                {
                    if loaves[index] == Entity::PLACEHOLDER {
                        found += 1;
                    }
                    loaves[index] = entity;
                }
            }
            if let Ok(entity_children) = children.get(entity) {
                stack.extend(entity_children.iter());
            }
        }
        if found == BAKERY_BREAD_MESH_COUNT {
            let visible = bakery_bread_level(inventory);
            for (index, loaf) in loaves.iter().enumerate() {
                if let Ok(mut current) = visibility.get_mut(*loaf) {
                    *current = if index < usize::from(visible) {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    };
                }
            }
            commands
                .entity(bakery)
                .insert(BakeryBreadDisplay { loaves, visible });
        }
    }
}

pub(super) fn bakery_bread_level(inventory: &GoodsInventory) -> u8 {
    let bread = inventory.amount(Good::Bread);
    if bread == 0 {
        return 0;
    }
    let maximum = (inventory.bulk_capacity() / Good::Bread.bulk_per_unit()).max(1);
    bread
        .saturating_mul(BAKERY_BREAD_MESH_COUNT as u32)
        .div_ceil(maximum)
        .clamp(1, BAKERY_BREAD_MESH_COUNT as u32) as u8
}

pub(super) fn sync_bakery_bread_display(
    mut commands: Commands,
    mut bakeries: Query<(Entity, &GoodsInventory, &mut BakeryBreadDisplay)>,
    mut visibility: Query<&mut Visibility>,
) {
    for (bakery, inventory, mut display) in bakeries.iter_mut() {
        if display
            .loaves
            .iter()
            .any(|loaf| visibility.get(*loaf).is_err())
        {
            // Scene streaming can replace descendants while retaining the
            // replicated root. Let setup discover the replacement nodes.
            commands.entity(bakery).remove::<BakeryBreadDisplay>();
            continue;
        }
        let visible = bakery_bread_level(inventory);
        if visible == display.visible {
            continue;
        }
        for (index, loaf) in display.loaves.iter().enumerate() {
            if let Ok(mut current) = visibility.get_mut(*loaf) {
                *current = if index < usize::from(visible) {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        }
        display.visible = visible;
    }
}
