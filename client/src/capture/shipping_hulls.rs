//! Fixed presentation inputs for the ordinary ship/pier/gangway renderer.
//! The Coaster uses an actual connected-lab berth. The offshore Cog is a mesh
//! specimen, not evidence that this Coaster port admits a Cog or that it sailed.

use super::{CaptureConfig, CaptureState};
use crate::settlement::shipping::{PortAssetReady, PortOfficeDoor, ShippingVisualSet};
use bevy::prelude::*;
use shared::{components::*, terrain::WorldTerrain};
use std::collections::HashSet;

const COASTER: Vec3 = Vec3::new(277.95, 0., 152.);
const COG: Vec3 = Vec3::new(184.95, 0., -150.);

#[derive(Resource, Default)]
pub(super) struct Review {
    staged: bool,
    door_cycle: bool,
    saw_open: bool,
    saw_closed_after: bool,
}
#[derive(Component)]
pub(super) struct ReviewHull;
#[derive(Component)]
pub(super) struct ReviewPort;
#[derive(Component)]
pub(super) struct ReviewVisitor;

pub(super) fn install(app: &mut App) {
    if std::env::var("FISTWORLD_CAPTURE_SHIPPING_HULLS").as_deref() != Ok("1") {
        return;
    }
    app.init_resource::<Review>();
    app.add_systems(
        Update,
        (stage, drive_visitor)
            .chain()
            .before(ShippingVisualSet)
            .run_if(in_state(crate::states::GameState::Playing)),
    );
}

fn depth_clear(terrain: &WorldTerrain, centre: Vec3, kind: ShipKind) -> bool {
    // Presentation fixture validation only. Ordinary server navigation also
    // certifies obstruction, overhead clearance and complete swept routes.
    let radius = match kind {
        ShipKind::Coaster => 3.2,
        ShipKind::Cog => 4.8,
    };
    (-5..=5).all(|x| {
        (-5..=5).all(|z| {
            let offset = Vec2::new(x as f32, z as f32) * radius / 5.;
            if offset.length_squared() > radius * radius {
                return true;
            }
            let point = centre.xz() + offset;
            terrain
                .get_water_height(point.x, point.y)
                .is_some_and(|water| {
                    water - terrain.get_height(point.x, point.y) >= kind.draft() + 0.12
                })
        })
    })
}

fn stage(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    manifest: Res<crate::hero::HeroManifest>,
    mut review: ResMut<Review>,
) {
    if review.staged {
        return;
    }
    assert_eq!(terrain.generator.active_map_id(), "village_lab");
    assert!(
        depth_clear(&terrain, COASTER, ShipKind::Coaster),
        "maintained Coaster berth changed"
    );
    assert!(
        depth_clear(&terrain, COG, ShipKind::Cog),
        "Cog specimen needs real deep water"
    );
    // Full landing/head/hull certified by the maintained server survey probe.
    let shore = Vec3::new(248., 1.8206187, 152.);
    assert!(shore.y > 0.4, "port foundation must be on dry ground");
    let geometry = PortGeometry {
        shore,
        pier_end: Vec3::new(274., 1., 152.),
        berth: COASTER,
        departure: Vec3::new(277.95, 0., 140.05),
        yaw: 0.,
        maximum_ship: ShipKind::Coaster,
    };
    assert!(geometry.valid());
    commands.spawn((
        ReviewPort,
        BuildingId(891_001),
        SettlementPort {
            settlement: SettlementId(891_001),
            geometry,
            built: true,
        },
    ));
    for (index, kind, point, yaw) in [
        (1, ShipKind::Coaster, COASTER, 0.),
        (2, ShipKind::Cog, COG, 0.),
    ] {
        commands.spawn((
            ReviewHull,
            Vessel,
            ShipId(891_000 + index),
            CompanyShip {
                company: CompanyId(891_001),
                kind,
                // The offshore specimen's home is outside this presentation
                // fixture; it is deliberately not admitted to the smaller port.
                home_port: BuildingId(891_000 + index),
                assigned_route: None,
                status: ShipStatus::Moored,
            },
            PlayerPosition(point),
            PlayerRotation(yaw),
            CharacterMotion::STATIONARY,
        ));
    }
    review.door_cycle = std::env::var("FISTWORLD_CAPTURE_PORT_DOOR").as_deref() == Ok("1");
    if review.door_cycle {
        let anchor = geometry.project_asset_point(Vec3::new(-5.25, 1., -1.55));
        commands.spawn((
            ReviewVisitor,
            PersonId(891_100),
            CharacterKind::Villager,
            CharacterName("Harbour visitor".into()),
            CharacterAffiliation::default(),
            HeroOutfit::from_manifest(&manifest),
            CharacterActivity::Idle,
            CharacterMotion::STATIONARY,
            PlayerPosition(anchor + Vec3::X * 4.),
            PlayerRotation(geometry.pier_yaw()),
        ));
    }
    review.staged = true;
}

// Presentation trajectory through the ordinary proximity/clip state machine.
// This never sets a door pose or claims server navigation.
fn drive_visitor(
    state: Res<CaptureState>,
    time: Res<Time>,
    mut review: ResMut<Review>,
    ports: Query<(&SettlementPort, Option<&PortOfficeDoor>), With<ReviewPort>>,
    mut visitor: Query<(&mut PlayerPosition, &mut CharacterMotion), With<ReviewVisitor>>,
) {
    if !review.door_cycle {
        return;
    }
    let Ok((port, door)) = ports.single() else {
        return;
    };
    if let Some(door) = door {
        review.saw_open |= door.is_open();
        review.saw_closed_after |= review.saw_open && door.is_shut();
    }
    let shot = match *state {
        CaptureState::Settling { shot, .. } | CaptureState::AwaitingCapture { shot, .. } => shot,
        _ => 0,
    };
    let distance = if shot < 30 {
        4.
    } else if shot < 60 {
        4. - (shot - 30) as f32 * 3.2 / 30.
    } else if shot < 125 {
        0.8
    } else if shot < 155 {
        0.8 + (shot - 125) as f32 * 3.2 / 30.
    } else {
        4.
    };
    let anchor = port
        .geometry
        .project_asset_point(Vec3::new(-5.25, 1., -1.55));
    let target =
        anchor + Quat::from_rotation_y(port.geometry.pier_yaw()) * Vec3::new(distance, 0., 0.);
    for (mut position, mut motion) in &mut visitor {
        motion.velocity = (target - position.0) / time.delta_secs().max(0.001);
        position.0 = target;
    }
    if shot >= 239 {
        assert!(
            review.saw_open && review.saw_closed_after,
            "proximity visitor must open the authored door and leave it shut again"
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn ready(
    review: Option<Res<Review>>,
    config: Res<CaptureConfig>,
    state: Res<CaptureState>,
    hulls: Query<
        (
            &CompanyShip,
            &Mesh3d,
            &MeshMaterial3d<StandardMaterial>,
            &Transform,
            &Visibility,
        ),
        With<ReviewHull>,
    >,
    ports: Query<(Entity, &SettlementPort, &PortAssetReady), With<ReviewPort>>,
    children: Query<&Children>,
    doors: Query<&PortOfficeDoor>,
    visitors: Query<(), (With<ReviewVisitor>, With<crate::hero::HeroDressed>)>,
    primitives: Query<(&Mesh3d, &MeshMaterial3d<StandardMaterial>)>,
    gangways: Query<(&Name, &ChildOf, &Mesh3d, &MeshMaterial3d<StandardMaterial>)>,
    meshes: Res<Assets<Mesh>>,
    materials: Res<Assets<StandardMaterial>>,
    mut waits: Local<u32>,
    mut saved: Local<HashSet<usize>>,
) -> bool {
    let Some(review) = review else {
        return true;
    };
    let mut ships = Vec::new();
    for (hull, mesh, material, transform, visibility) in &hulls {
        let Some(mesh) = meshes.get(&mesh.0) else {
            continue;
        };
        if !materials.contains(&material.0) || *visibility == Visibility::Hidden {
            continue;
        }
        ships.push(serde_json::json!({
            "kind":hull.kind,"position":transform.translation.to_array(),
            "vertices":mesh.count_vertices(),"indices":mesh.indices().map_or(0,|i|i.len()),
            "length":hull.kind.length(),"beam":hull.kind.beam(),"mast":hull.kind.mast_height(),
        }));
    }
    let port = ports.single().ok().filter(|(entity, _, ready)| {
        ready.meshes == 10
            && ready.anchors == 12
            && ready.door_clips == 2
            && children
                .iter_descendants(*entity)
                .filter_map(|e| primitives.get(e).ok())
                .all(|(mesh, material)| meshes.contains(&mesh.0) && materials.contains(&material.0))
    });
    let gangway_ready = port.is_some_and(|(entity, ..)| {
        gangways.iter().any(|(name, parent, mesh, material)| {
            name.as_str() == "Removable ship gangway"
                && parent.parent() == entity
                && meshes.contains(&mesh.0)
                && materials.contains(&material.0)
        })
    });
    let ready = review.staged
        && ships.len() == 2
        && port.is_some()
        && gangway_ready
        && (!review.door_cycle || visitors.iter().count() == 1);
    if !ready {
        *waits += 1;
        assert!(
            *waits < 1800,
            "shipping fixture did not instantiate both real hulls, port and gangway"
        );
        return false;
    }
    *waits = 0;
    let CaptureState::Settling { shot, .. } = *state else {
        return true;
    };
    if saved.insert(shot) {
        let name = &config.shots[shot].name;
        let (port_entity, port, ready) = port.unwrap();
        let evidence = serde_json::json!({
            "fixture":"production shipping hull/pier presentation","shot":name,
            "scope":"Static replicated inputs, actual meshes and water buoyancy. No sailing, procurement or Cog berth admission is claimed.",
            "hulls":ships,"port":port,"port_vertices":ready.vertices, "port_meshes":ready.meshes, "port_anchors":ready.anchors,
            "port_door_clips":ready.door_clips, "port_asset":"game_assets/buildings/ports/Port.glb",
            "port_indices":ready.indices,"actual_gangway_ready":gangway_ready,
            "door_phase":doors.get(port_entity).ok().map(|d|d.phase()),
            "door_cycle_open_seen":review.saw_open,"door_cycle_closed_after_seen":review.saw_closed_after,
        });
        std::fs::create_dir_all(&config.out_dir).expect("shipping capture directory");
        std::fs::write(
            config.out_dir.join(format!("{name}.shipping.json")),
            serde_json::to_vec_pretty(&evidence).unwrap(),
        )
        .expect("shipping geometry evidence");
    }
    true
}
