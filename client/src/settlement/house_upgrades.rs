//! Temporary, ground-supported scaffold around an existing occupied home.

use bevy::prelude::*;
use shared::components::{ConstructionSite, HouseUpgradeWorksite, PlayerPosition};
use shared::terrain::WorldTerrain;

#[derive(Component)]
pub(super) struct HouseUpgradeScaffold;

/// One cached cuboid/material lets Bevy batch the few temporary timber parts.
/// The front remains completely open for the existing door and household route.
pub(super) fn attach_house_upgrade_scaffolds(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    terrain: Option<Res<WorldTerrain>>,
    mut assets: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
    sites: Query<
        (
            Entity,
            &HouseUpgradeWorksite,
            &ConstructionSite,
            &PlayerPosition,
        ),
        Without<HouseUpgradeScaffold>,
    >,
) {
    let Some(terrain) = terrain else {
        return;
    };
    if sites.is_empty() {
        return;
    }
    let (mesh, material) = assets
        .get_or_insert_with(|| {
            (
                meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.34, 0.22, 0.12),
                    perceptual_roughness: 0.96,
                    ..default()
                }),
            )
        })
        .clone();
    for (entity, upgrade, site, position) in &sites {
        let half = upgrade.target.building_type().definition().footprint * 0.5;
        let platform = 3.15;
        let rail = platform + 0.9;
        let extent = (half.y - 0.35).max(1.0);
        let rotation = Quat::from_rotation_y(site.rotation);
        let ground = |x: f32, z: f32| {
            let at = position.0 + rotation * Vec3::new(x, 0.0, z);
            terrain.get_height(at.x, at.z) - position.0.y
        };
        commands
            .entity(entity)
            .insert((
                HouseUpgradeScaffold,
                Transform::from_translation(position.0).with_rotation(rotation),
                Visibility::Inherited,
            ))
            .with_children(|parent| {
                for side in [-1.0, 1.0] {
                    let inner = side * (half.x + 0.45);
                    let outer = side * (half.x + 1.15);
                    let center = (inner + outer) * 0.5;
                    for x in [inner, outer] {
                        for z in [-extent, extent] {
                            let base = ground(x, z) - 0.08;
                            part(
                                parent,
                                &mesh,
                                &material,
                                "Scaffold upright",
                                Vec3::new(x, (base + rail + 0.08) * 0.5, z),
                                Vec3::new(0.14, rail + 0.08 - base, 0.14),
                                Quat::IDENTITY,
                            );
                        }
                    }
                    for z in [-extent, extent] {
                        part(
                            parent,
                            &mesh,
                            &material,
                            "Platform bearer",
                            Vec3::new(center, platform - 0.07, z),
                            Vec3::new(0.94, 0.14, 0.16),
                            Quat::IDENTITY,
                        );
                        part(
                            parent,
                            &mesh,
                            &material,
                            "Scaffold end rail",
                            Vec3::new(center, rail, z),
                            Vec3::new(0.84, 0.10, 0.10),
                            Quat::IDENTITY,
                        );
                    }
                    part(
                        parent,
                        &mesh,
                        &material,
                        "Supported scaffold platform",
                        Vec3::new(center, platform + 0.05, 0.0),
                        Vec3::new(0.84, 0.10, extent * 2.0 + 0.16),
                        Quat::IDENTITY,
                    );
                    part(
                        parent,
                        &mesh,
                        &material,
                        "Scaffold outer rail",
                        Vec3::new(outer, rail, 0.0),
                        Vec3::new(0.10, 0.10, extent * 2.0),
                        Quat::IDENTITY,
                    );
                    for (from, to) in [(-extent, extent), (extent, -extent)] {
                        let a = Vec3::new(outer, ground(outer, from) + 0.35, from);
                        let b = Vec3::new(outer, platform - 0.07, to);
                        let span = b - a;
                        part(
                            parent,
                            &mesh,
                            &material,
                            "Joined scaffold brace",
                            (a + b) * 0.5,
                            Vec3::new(0.10, span.length(), 0.10),
                            Quat::from_rotation_arc(Vec3::Y, span.normalize()),
                        );
                    }
                }
            });
    }
}

fn part(
    parent: &mut ChildSpawnerCommands<'_>,
    mesh: &Handle<Mesh>,
    material: &Handle<StandardMaterial>,
    name: &'static str,
    at: Vec3,
    size: Vec3,
    rotation: Quat,
) {
    parent.spawn((
        Name::new(name),
        Mesh3d(mesh.clone()),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(at)
            .with_scale(size)
            .with_rotation(rotation),
    ));
}
