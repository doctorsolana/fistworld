//! Offline selection states through the production range renderer. This proves
//! presentation and shared range values, not authoritative firing or clear shots.

use super::{CaptureConfig, CaptureState};
use bevy::prelude::*;
use shared::components::*;
use shared::terrain::WorldTerrain;

#[derive(Resource, Default)]
pub(super) struct ArcherRangeStudy {
    archers: Vec<Entity>,
    detached: Option<Entity>,
    phase: Option<usize>,
    ready: bool,
    waiting: usize,
}

pub(super) fn install(app: &mut App) {
    if std::env::var("FISTFORCE_CAPTURE_ARCHER_RANGE").as_deref() != Ok("1") {
        return;
    }
    app.init_resource::<ArcherRangeStudy>()
        .add_systems(PostStartup, stage)
        .add_systems(PreUpdate, prepare)
        .add_systems(Last, inspect);
}

pub(super) fn ready(study: Option<Res<ArcherRangeStudy>>) -> bool {
    study.is_none_or(|study| study.ready)
}

fn stage(world: &mut World) {
    let focus = world.resource::<CaptureConfig>().shots[0].focus;
    let manifest =
        shared::character::CharacterManifest::load().expect("archer range fixture wardrobe");
    let mut outfit = HeroOutfit::from_manifest(&manifest);
    manifest
        .apply_outfit("soldier_padded", &mut outfit)
        .unwrap();
    world.insert_resource(crate::ui::name_entry::PlayerNameInput {
        name: "range-review".into(),
        submitted: true,
    });
    world.insert_resource(crate::combat_mode::CombatMode(false));
    world.spawn((
        Battalion {
            id: BattalionId(9001),
            name: "I · Archers".into(),
            ordinal: 1,
        },
        CommandedBy("range-review".into()),
        SoldierRole::Archer,
    ));
    for index in 0..17 {
        let offset = if index == 16 {
            Vec3::new(14.0, 0.0, 0.0)
        } else {
            Vec3::new(
                (index % 4) as f32 * 2.0 - 3.0,
                0.0,
                (index / 4) as f32 * 2.0 - 3.0,
            )
        };
        let mut at = focus + offset;
        at.y = world.resource::<WorldTerrain>().get_height(at.x, at.z);
        let entity = world
            .spawn((
                PersonId(990_000 + index),
                CharacterName(if index == 16 {
                    "Detached archer".into()
                } else {
                    format!("Archer {}", index + 1)
                }),
                CharacterKind::Villager,
                CharacterAffiliation::default(),
                CommandedBy("range-review".into()),
                CharacterAttributes::default(),
                outfit.clone(),
                CharacterActivity::Idle,
                CharacterMotion::STATIONARY,
                PlayerPosition(at),
                PlayerRotation(std::f32::consts::PI),
                SoldierRole::Archer,
                Quiver::default(),
                Health::new(100.0),
                BowEquipped,
            ))
            .id();
        if index == 16 {
            world.resource_mut::<ArcherRangeStudy>().detached = Some(entity);
        } else {
            world
                .entity_mut(entity)
                .insert(MemberOfBattalion(BattalionId(9001)));
            world
                .resource_mut::<ArcherRangeStudy>()
                .archers
                .push(entity);
        }
    }
}

fn prepare(world: &mut World) {
    let phase = match *world.resource::<CaptureState>() {
        CaptureState::Warmup { .. } => {
            let shot = &world.resource::<CaptureConfig>().shots[0];
            let (focus, yaw, zoom) = (shot.focus, shot.yaw, shot.zoom);
            for mut camera in world
                .query::<&mut crate::camera_rts::CommanderCamera>()
                .iter_mut(world)
            {
                camera.focus = focus;
                camera.focus_target = focus;
                camera.yaw = yaw;
                camera.yaw_target = yaw;
                camera.zoom = zoom;
                camera.zoom_target = zoom;
            }
            0
        }
        CaptureState::Settling { shot, .. } => shot,
        _ => return,
    };
    if world.resource::<ArcherRangeStudy>().phase == Some(phase) {
        return;
    }
    let first = world.resource::<ArcherRangeStudy>().archers[0];
    let detached = world.resource::<ArcherRangeStudy>().detached.unwrap();
    let selected = match phase {
        1 => vec![first], // Ordinary selection expansion must select all 16 members.
        2 | 3 | 4 => vec![detached],
        _ => Vec::new(),
    };
    if phase == 3 {
        let mut at = world.get::<PlayerPosition>(detached).unwrap().0 + Vec3::new(-24.0, 0.0, 12.0);
        at.y = world.resource::<WorldTerrain>().get_height(at.x, at.z);
        world.get_mut::<PlayerPosition>(detached).unwrap().0 = at;
    }
    if phase == 4 {
        world.entity_mut(detached).remove::<BowEquipped>();
    }
    world
        .resource_mut::<crate::selection::Selection>()
        .set(selected);
    let mut study = world.resource_mut::<ArcherRangeStudy>();
    study.phase = Some(phase);
    study.ready = false;
    study.waiting = 0;
}

fn inspect(world: &mut World) {
    let study = world.resource::<ArcherRangeStudy>();
    if study.ready || study.phase.is_none() {
        return;
    }
    let phase = study.phase.unwrap();
    let archers = study.archers.clone();
    let detached = study.detached.unwrap();
    let dressed = archers
        .iter()
        .chain(std::iter::once(&detached))
        .all(|person| {
            world.get::<crate::hero::HeroDressed>(*person).is_some()
                && (world.get::<BowEquipped>(*person).is_none()
                    || world.get::<crate::hero::BowDressed>(*person).is_some())
        });
    let expected = usize::from((1..=3).contains(&phase));
    let state = world.resource::<crate::selection::archer_range::ArcherRangeState>();
    let count_ready =
        state.ranges.len() == expected && (phase != 1 || state.ranges[0].sources == 16);
    let centred = if phase == 2 || phase == 3 {
        state.ranges.first().is_some_and(|range| {
            let position = world.get::<PlayerPosition>(detached).unwrap().0;
            range.centre.xz().distance_squared(position.xz()) < 0.02
        })
    } else {
        true
    };
    let study = &mut *world.resource_mut::<ArcherRangeStudy>();
    study.waiting += 1;
    assert!(
        study.waiting < 1800,
        "archer range presentation did not become ready"
    );
    if !dressed || !count_ready || !centred || study.waiting < 20 {
        return;
    }
    study.ready = true;
    let terrain = world.resource::<WorldTerrain>();
    let state = world.resource::<crate::selection::archer_range::ArcherRangeState>();
    let ranges: Vec<_> = state.ranges.iter().map(|range| {
        let error = range.points.iter().map(|point| (point.xz().distance(range.centre.xz()) - BOW_RANGE).abs()).fold(0.0, f32::max);
        let lifts: Vec<_> = range.points.iter().map(|point| point.y - terrain.get_height(point.x, point.z)).collect();
        assert!(error < 0.001, "range must use the shared firing distance");
        assert!(lifts.iter().all(|lift| (*lift - 0.18).abs() < 0.001), "range must follow terrain");
        serde_json::json!({"centre":range.centre.to_array(),"sources":range.sources,"radius":BOW_RANGE,
            "vertices":range.points.len(),"radius_error":error,"minimum_ground_lift":lifts.iter().copied().fold(f32::INFINITY,f32::min),
            "maximum_ground_lift":lifts.iter().copied().fold(f32::NEG_INFINITY,f32::max)})
    }).collect();
    assert!(
        !world.resource::<crate::combat_mode::CombatMode>().0,
        "range must work in normal Play"
    );
    let evidence = serde_json::json!({
        "scope":"offline production selection/range presentation; no claim of a clear firing trajectory",
        "phase":phase,"normal_play":true,"selection_count":world.resource::<crate::selection::Selection>().entities.len(),
        "expected_range_count":expected,"ranges":ranges,
    });
    let out = world
        .resource::<CaptureConfig>()
        .out_dir
        .join(format!("phase-{phase}.range.json"));
    std::fs::write(out, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
}
