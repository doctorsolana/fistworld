//! Worksite supply bundles and staged construction presentation.

use super::buildings::BuildingVisual;
use bevy::prelude::*;
use shared::components::{
    CivicHallUpgradeWorksite, ConstructionSite, HouseAppearance, PlayerPosition,
};
use shared::economy::{Good, GoodsInventory};
use shared::terrain::WorldTerrain;

#[derive(Component)]
pub(super) struct ConstructionSupplyVisual;

#[derive(Component)]
pub(super) struct ConstructionSupplyBundle {
    pub(super) site: Entity,
    pub(super) unit: u32,
    pub(super) good: Good,
}

/// Draw one physical bundle or dressed-stone block per delivered material unit
/// while a site waits. The Hall-upgrade marker switches the generic worksite
/// from Wood to Stone without inventing a parallel client-only construction
/// state.
pub(super) fn attach_construction_supply_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<
        Option<(
            Handle<Mesh>,
            Handle<StandardMaterial>,
            Handle<StandardMaterial>,
        )>,
    >,
    sites: Query<
        (
            Entity,
            &ConstructionSite,
            &PlayerPosition,
            &GoodsInventory,
            Option<&CivicHallUpgradeWorksite>,
            Option<&HouseAppearance>,
        ),
        Without<ConstructionSupplyVisual>,
    >,
) {
    let (mesh, wood_material, stone_material) = assets
        .get_or_insert_with(|| {
            (
                meshes.add(Cuboid::new(0.52, 0.24, 0.28)),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.30, 0.13, 0.045),
                    perceptual_roughness: 0.92,
                    ..default()
                }),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.38, 0.40, 0.42),
                    perceptual_roughness: 0.96,
                    ..default()
                }),
            )
        })
        .clone();

    for (entity, site, position, inventory, hall_upgrade, house) in sites.iter() {
        let (required, good, material, art) = hall_upgrade.map_or_else(
            || {
                (
                    site.kind.construction_wood_required(),
                    Good::Wood,
                    wood_material.clone(),
                    site.kind.art_with_house(house),
                )
            },
            |upgrade| {
                (
                    upgrade.material_required,
                    upgrade.material,
                    stone_material.clone(),
                    upgrade.target.building_type(),
                )
            },
        );
        let delivered = inventory.amount(good);
        commands.entity(entity).insert((
            ConstructionSupplyVisual,
            Transform::from_translation(position.0)
                .with_rotation(Quat::from_rotation_y(site.rotation)),
            Visibility::Inherited,
        ));
        let front = -(art.definition().footprint.y * 0.5 + 1.6);
        let half = art.definition().footprint * 0.5;
        commands.entity(entity).with_children(|parent| {
            for (index, corner) in [
                Vec2::new(-half.x, -half.y),
                Vec2::new(half.x, -half.y),
                Vec2::new(-half.x, half.y),
                Vec2::new(half.x, half.y),
            ]
            .into_iter()
            .enumerate()
            {
                parent.spawn((
                    Name::new(format!("Worksite stake {}", index + 1)),
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(wood_material.clone()),
                    Transform::from_xyz(corner.x, 0.36, corner.y)
                        .with_scale(Vec3::new(0.18, 3.0, 0.30)),
                ));
            }
            for unit in 1..=required {
                let index = unit - 1;
                let column = index % 3;
                let row = (index / 3) % 2;
                let layer = index / 6;
                parent.spawn((
                    Name::new(format!("Delivered {} {unit}", good.label())),
                    ConstructionSupplyBundle {
                        site: entity,
                        unit,
                        good,
                    },
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_xyz(
                        (column as f32 - 1.0) * 0.58,
                        0.14 + layer as f32 * 0.25,
                        front + row as f32 * 0.34,
                    ),
                    if !site.raising && unit <= delivered {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    },
                ));
            }
        });
    }
}

pub(super) fn sync_construction_supply_visuals(
    sites: Query<(&ConstructionSite, &GoodsInventory)>,
    mut bundles: Query<(&ConstructionSupplyBundle, &mut Visibility)>,
) {
    for (bundle, mut visibility) in bundles.iter_mut() {
        let Ok((site, inventory)) = sites.get(bundle.site) else {
            continue;
        };
        let next = if !site.raising && bundle.unit <= inventory.amount(bundle.good) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != next {
            *visibility = next;
        }
    }
}

/// A building part-way out of the ground, with its own clock.
#[derive(Component)]
pub(super) struct RaisingVisual {
    pub(super) elapsed: f32,
    /// How far it started below the ground, so the lerp has a floor.
    pub(super) sunk: f32,
    pub(super) resting_y: f32,
}

/// Draw a building rising out of its plot while it is being raised.
///
/// The server sends ONE bit — `raising` flips true when the ground is cleared —
/// and the clock runs here. Streaming a progress float instead would resend
/// every nearby site at tick rate even though region-scoped detail only needs
/// the transition.
/// The cost of the local clock is that it starts a network hop late, which at
/// ten seconds nobody can see.
pub(super) fn raise_construction_visuals(
    mut commands: Commands,
    time: Res<Time>,
    warp: Query<&shared::components::TimeWarp>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    terrain: Option<Res<WorldTerrain>>,
    mut sites: Query<(
        Entity,
        &ConstructionSite,
        &PlayerPosition,
        Option<&CivicHallUpgradeWorksite>,
        Option<&HouseAppearance>,
        Option<&mut RaisingVisual>,
        Option<&BuildingVisual>,
    )>,
    mut transforms: Query<&mut Transform>,
) {
    let Some(terrain) = terrain else {
        return;
    };
    let warp = warp.iter().next().map(|warp| warp.0).unwrap_or(1.0);
    for (entity, site, position, hall_upgrade, house, raising, drawn) in sites.iter_mut() {
        if !site.raising {
            continue;
        }
        let ground = terrain.get_height(position.0.x, position.0.z);
        let Some(mut raising) = raising else {
            // First frame of the raise: put the model in, fully underground.
            let art = hall_upgrade
                .map(|upgrade| upgrade.target.building_type())
                .unwrap_or_else(|| site.kind.art_with_house(house));
            let definition = art.definition();
            let sunk = definition.height.max(1.0);
            if drawn.is_none() {
                let resting_y = if art.scene_path().is_some() {
                    ground
                } else {
                    ground + definition.height * 0.5
                };
                let common = (
                    BuildingVisual { building_type: art },
                    RaisingVisual {
                        elapsed: 0.0,
                        sunk,
                        resting_y,
                    },
                    Name::new(format!("{} rising", site.kind.label())),
                    Transform::from_xyz(position.0.x, resting_y - sunk, position.0.z)
                        .with_rotation(Quat::from_rotation_y(site.rotation)),
                    Visibility::Inherited,
                );
                if let Some(scene) = art.scene_path() {
                    commands
                        .entity(entity)
                        .insert((common, WorldAssetRoot(asset_server.load(scene))));
                } else {
                    let mesh = meshes.add(Cuboid::new(
                        definition.footprint.x,
                        definition.height,
                        definition.footprint.y,
                    ));
                    let material = materials.add(StandardMaterial {
                        base_color: definition.color,
                        perceptual_roughness: 0.92,
                        ..default()
                    });
                    commands.entity(entity).insert((
                        common,
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                    ));
                }
            }
            continue;
        };

        raising.elapsed += time.delta_secs() * warp;
        let t = (raising.elapsed / shared::components::SETTLEMENT_RAISE_SECONDS).clamp(0.0, 1.0);
        // Ease out: it breaks ground quickly and settles, which reads as being
        // pushed up rather than as a linear lift.
        let eased = 1.0 - (1.0 - t) * (1.0 - t);
        if let Ok(mut transform) = transforms.get_mut(entity) {
            transform.translation.y = raising.resting_y - raising.sunk * (1.0 - eased);
        }
    }
}
