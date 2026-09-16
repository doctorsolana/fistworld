//! Cached low-poly hulls and authored, terrain-grounded public harbour scenes.

use bevy::prelude::*;
use shared::components::*;
mod port_asset;
use super::structure_mesh::StructureMesh;
use crate::states::GameState;
pub(crate) use port_asset::{OfficeDoor as PortOfficeDoor, PortAssetReady};

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ShippingVisualSet;

pub(super) struct ShippingVisualPlugin;
impl Plugin for ShippingVisualPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShipMeshes>();
        port_asset::install(app);
        app.add_systems(
            Update,
            (
                attach_ships,
                port_asset::attach_ports,
                port_asset::bind_scene,
                sync_gangways,
                port_asset::animate_offices,
            )
                .chain()
                .in_set(ShippingVisualSet)
                .run_if(in_state(GameState::Playing)),
        );
    }
}

#[derive(Resource, Default)]
struct ShipMeshes {
    hulls: [Option<Handle<Mesh>>; 2],
    material: Option<Handle<StandardMaterial>>,
}
#[derive(Component)]
struct ShipVisual;
#[derive(Component)]
pub(crate) struct PortVisual {
    snapshot: SettlementPort,
    scene: Option<Entity>,
}

fn attach_ships(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: ResMut<ShipMeshes>,
    ships: Query<(Entity, &CompanyShip, &PlayerPosition, &PlayerRotation), Without<ShipVisual>>,
) {
    for (entity, ship, position, rotation) in ships.iter().take(4) {
        let material = cache
            .material
            .get_or_insert_with(|| {
                materials.add(StandardMaterial {
                    perceptual_roughness: 0.92,
                    ..default()
                })
            })
            .clone();
        let mesh = cache.hulls[ship.kind as usize]
            .get_or_insert_with(|| meshes.add(hull_mesh(ship.kind)))
            .clone();
        commands.entity(entity).insert((
            ShipVisual,
            crate::boat::BoatVisual {
                snapshot: position.0,
            },
            Name::new(ship.kind.label()),
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(position.0)
                .with_rotation(Quat::from_rotation_y(rotation.0)),
            Visibility::Inherited,
        ));
    }
}

/// The removable boarding plank exists only while a hull is alongside the pier.
#[derive(Component)]
struct Gangway {
    ship: ShipId,
    entity: Entity,
}

fn sync_gangways(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    cache: Res<ShipMeshes>,
    ports: Query<(
        Entity,
        &SettlementPort,
        Option<&Gangway>,
        Option<&PortAssetReady>,
    )>,
    ships: Query<(&ShipId, &CompanyShip, &PlayerPosition, &PlayerRotation)>,
) {
    let Some(material) = cache.material.as_ref() else {
        return;
    };
    for (entity, port, previous, ready) in &ports {
        let alongside = ships.iter().find(|(_, ship, position, _)| {
            port.built
                && ready.is_some()
                && ship.status != ShipStatus::Sailing
                && position.0.xz().distance_squared(port.geometry.berth.xz()) < 0.36
        });
        if previous.is_some_and(|old| alongside.is_some_and(|(id, _, _, _)| old.ship == *id)) {
            continue;
        }
        if let Some(old) = previous {
            commands.entity(old.entity).despawn();
            commands.entity(entity).remove::<Gangway>();
        }
        let Some((id, hull, position, yaw)) = alongside else {
            continue;
        };
        let start = port.geometry.pier_end;
        let end = position.0
            + Quat::from_rotation_y(yaw.0) * Vec3::new(0.0, 0.65, hull.kind.length() * 0.28);
        let mut mesh = StructureMesh::default();
        mesh.beam(
            start - Vec3::Y * 0.05,
            end - Vec3::Y * 0.05,
            0.80,
            0.10,
            Vec3::new(0.55, 0.37, 0.18),
        );
        let child = commands
            .spawn((
                Name::new("Removable ship gangway"),
                Mesh3d(meshes.add(mesh.finish())),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(
                    Quat::from_rotation_y(-port.geometry.pier_yaw()) * -port.geometry.shore,
                )
                .with_rotation(Quat::from_rotation_y(-port.geometry.pier_yaw())),
                ChildOf(entity),
                Visibility::Inherited,
            ))
            .id();
        commands.entity(entity).insert(Gangway {
            ship: *id,
            entity: child,
        });
    }
}

pub(crate) fn hull_mesh(kind: ShipKind) -> Mesh {
    let mut mesh = StructureMesh::default();
    let length = kind.length();
    let half_beam = kind.beam() * 0.5;
    let wood = Vec3::new(0.38, 0.24, 0.12);
    let trim = Vec3::new(0.59, 0.40, 0.20);
    let cream = Vec3::new(0.86, 0.79, 0.57);
    // Narrow, raised bow and stern; broad centre. Four closed strakes per side
    // give the hull volume rather than an opaque box or a see-through shell.
    let stations = [
        (-0.50, 0.08),
        (-0.36, 0.70),
        (-0.14, 1.0),
        (0.22, 0.97),
        (0.43, 0.63),
        (0.50, 0.30),
    ];
    for pair in stations.windows(2) {
        let [(za, wa), (zb, wb)] = [pair[0], pair[1]];
        let point = |z: f32, w: f32, level: usize, sign: f32| {
            let (y, taper) = [
                (-kind.draft(), 0.17),
                (-0.24, 0.78),
                (0.38, 1.0),
                (0.80, 1.02),
            ][level];
            Vec3::new(w * half_beam * taper * sign, y, z * length)
        };
        for sign in [-1.0, 1.0] {
            for band in 0..3 {
                let a = point(za, wa, band, sign);
                let b = point(zb, wb, band, sign);
                let c = point(zb, wb, band + 1, sign);
                let d = point(za, wa, band + 1, sign);
                if sign > 0.0 {
                    mesh.quad(d, c, b, a, wood * (0.92 + band as f32 * 0.08));
                } else {
                    mesh.quad(a, b, c, d, wood * (0.92 + band as f32 * 0.08));
                }
                // Inward faces of the upper gunwale are visible above deck.
                if band == 2 {
                    if sign > 0.0 {
                        mesh.quad(a, b, c, d, trim);
                    } else {
                        mesh.quad(d, c, b, a, trim);
                    }
                }
            }
            mesh.beam(
                point(za, wa, 3, sign),
                point(zb, wb, 3, sign),
                0.10,
                0.10,
                trim,
            );
        }
        mesh.quad(
            point(za, wa, 0, 1.0),
            point(zb, wb, 0, 1.0),
            point(zb, wb, 0, -1.0),
            point(za, wa, 0, -1.0),
            wood * 0.7,
        );
        let deck = |z, w, sign| Vec3::new(w * half_beam * sign, 0.64, z * length);
        mesh.quad(
            deck(za, wa, -1.0),
            deck(zb, wb, -1.0),
            deck(zb, wb, 1.0),
            deck(za, wa, 1.0),
            trim,
        );
    }
    for (z, width) in [stations[0], stations[5]] {
        let top = 0.80;
        let bottom = -kind.draft();
        let side = width * half_beam;
        let a = Vec3::new(-side * 0.17, bottom, z * length);
        let b = Vec3::new(side * 0.17, bottom, z * length);
        let c = Vec3::new(side, top, z * length);
        let d = Vec3::new(-side, top, z * length);
        if z < 0.0 {
            mesh.quad(d, c, b, a, wood);
        } else {
            mesh.quad(a, b, c, d, wood);
        }
    }
    // Deck boards and hatch sit on the actual 64 cm walking deck.
    for i in 0..9 {
        let z = (-0.28 + i as f32 * 0.07) * length;
        mesh.beam(
            Vec3::new(-half_beam * 0.72, 0.645, z),
            Vec3::new(half_beam * 0.72, 0.645, z),
            0.022,
            0.013,
            wood,
        );
    }
    mesh.beam(
        Vec3::new(-0.43 * half_beam, 0.74, length * 0.04),
        Vec3::new(0.43 * half_beam, 0.74, length * 0.04),
        length * 0.17,
        0.18,
        wood * 0.90,
    );
    let mast_z = -length * 0.13;
    let mast_top = kind.mast_height();
    mesh.beam(
        Vec3::new(0.0, 0.64, mast_z),
        Vec3::new(0.0, mast_top, mast_z),
        0.14,
        0.14,
        wood,
    );
    let yard_y = mast_top - 0.30;
    let sail_bottom = 1.30;
    mesh.beam(
        Vec3::new(-half_beam * 0.90, yard_y, mast_z),
        Vec3::new(half_beam * 0.90, yard_y, mast_z),
        0.085,
        0.085,
        trim,
    );
    // Faceted cream cloth has real front and reverse surfaces; no alpha blend.
    for side in [-1.0, 1.0] {
        let a = Vec3::new(0.0, yard_y - 0.04, mast_z - 0.07);
        let b = Vec3::new(half_beam * 0.85 * side, yard_y - 0.04, mast_z - 0.04);
        let c = Vec3::new(half_beam * 0.65 * side, sail_bottom, mast_z - 0.22);
        let d = Vec3::new(0.0, sail_bottom + 0.04, mast_z - 0.40);
        mesh.quad(a, b, c, d, cream * if side > 0.0 { 1.0 } else { 0.96 });
        mesh.quad(d, c, b, a, cream * 0.91);
        mesh.beam(
            Vec3::new(0.0, mast_top - 0.15, mast_z),
            Vec3::new(half_beam * 0.85 * side, 0.80, length * 0.21),
            0.026,
            0.026,
            cream * 0.64,
        );
    }
    // Rudder and tiller meet; both are below the helmsman's hands.
    mesh.beam(
        Vec3::new(0.0, -kind.draft() * 0.75, length * 0.51),
        Vec3::new(0.0, 0.91, length * 0.51),
        0.12,
        0.40,
        wood,
    );
    mesh.beam(
        Vec3::new(0.0, 0.92, length * 0.51),
        Vec3::new(0.0, 0.92, length * 0.26),
        0.09,
        0.09,
        trim,
    );
    mesh.finish()
}
