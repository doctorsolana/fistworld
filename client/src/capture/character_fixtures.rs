//! Deterministic inputs for the production character renderer/animation driver.
//! No direct bone poses: the same activity, motion and combat components as replication.
use super::{CaptureConfig, CaptureState};
use bevy::prelude::*;
use shared::character::CharacterManifest;
use shared::components::*;
use shared::terrain::WorldTerrain;

#[derive(Component)]
pub(super) struct CharacterReview {
    index: usize,
    mode: String,
    origin: Vec3,
}

pub(super) fn stage(
    mut commands: Commands,
    mut config: ResMut<CaptureConfig>,
    terrain: Option<Res<WorldTerrain>>,
    mut settings: ResMut<crate::render::systems::GraphicsSettings>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Ok(mode) = std::env::var("FISTFORCE_CAPTURE_CHARACTER_REVIEW") else {
        *done = true;
        return;
    };
    settings.props_enabled = false;
    let Some(terrain) = terrain else {
        return;
    };
    let manifest = CharacterManifest::load().expect("character review manifest");
    let focus = config.shots.first().expect("character review shot").focus;
    let count = match mode.as_str() {
        "deaths" | "swim" | "motion" => 2,
        "rest" => 1,
        _ => 3,
    };
    for index in 0..count {
        let x = focus.x + (index as f32 - (count - 1) as f32 * 0.5) * 2.6;
        let y = if mode == "swim" {
            shared::character::locomotion::swimming_surface(
                terrain.get_height(x, focus.z),
                terrain.get_water_height(x, focus.z),
            )
            .expect("swim review requires deep water")
        } else {
            terrain.get_height(x, focus.z)
        };
        let origin = Vec3::new(x, y, focus.z);
        if mode == "swim" {
            for shot in &mut config.shots {
                shot.focus.y = y;
            }
        }
        let mut outfit = HeroOutfit::from_manifest(&manifest);
        if mode == "armour" || mode == "deaths" || mode == "archery" {
            let preset = if mode == "deaths" {
                "soldier_mail"
            } else {
                ["soldier_padded", "soldier_leather", "soldier_mail"][index]
            };
            manifest
                .apply_outfit(preset, &mut outfit)
                .expect("review equipment preset");
        } else if mode == "motion" {
            outfit.slots[0] = 2;
            outfit.slots[1] = 3;
            outfit.slots[2] = 6;
        }
        let entity = commands
            .spawn((
                Hero {
                    owner: lightyear::prelude::PeerId::Netcode(80_000 + index as u64),
                },
                PersonId(80_000 + index as u64),
                CharacterName(format!("Review {mode} {index}")),
                CharacterKind::Hero,
                CharacterAffiliation::default(),
                outfit,
                if mode == "deaths" {
                    CharacterActivity::Fighting
                } else {
                    CharacterActivity::Idle
                },
                CharacterMotion::STATIONARY,
                PlayerPosition(origin),
                PlayerRotation(if mode == "archery" {
                    [std::f32::consts::PI, 2.1, 4.4][index]
                } else {
                    std::f32::consts::PI
                }),
                CharacterReview {
                    index,
                    mode: mode.clone(),
                    origin,
                },
            ))
            .id();
        if mode == "archery" {
            commands
                .entity(entity)
                .insert(shared::components::BowEquipped);
        }
    }
    *done = true;
}

pub(super) fn drive(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<CaptureState>,
    clocks: Query<&WorldTime>,
    mut elapsed: Local<f32>,
    mut people: Query<
        (
            Entity,
            &CharacterReview,
            &mut CharacterActivity,
            &mut CharacterMotion,
            &mut PlayerPosition,
        ),
        With<crate::hero::HeroDressed>,
    >,
    pending: Query<(), (With<CharacterReview>, Without<crate::hero::HeroDressed>)>,
) {
    // Only exercise transient actions once the full wardrobe is dressed and
    // the capture harness has established terrain readiness.
    if !pending.is_empty() || matches!(*state, CaptureState::Warmup { .. }) {
        return;
    }
    *elapsed += time.delta_secs();
    let now = clocks.iter().next().map_or(0., |c| {
        f64::from(c.day) * f64::from(c.cycle_duration()) + f64::from(c.seconds_in_cycle)
    });
    for (entity, review, mut activity, mut motion, mut position) in &mut people {
        match review.mode.as_str() {
            "work" => {
                *activity = [
                    CharacterActivity::Building,
                    CharacterActivity::Chopping,
                    CharacterActivity::Farming,
                ][review.index];
            }
            "archery" => {
                commands.entity(entity).insert(shared::components::BowShot {
                    release_at: now - f64::from(*elapsed) + 1.0,
                });
            }
            "rest" => {
                *activity = CharacterActivity::LyingDown;
            }
            "deaths" if *elapsed > 0.5 => {
                commands.entity(entity).insert(CombatReaction {
                    at: now - f64::from(*elapsed - 0.5),
                    fatal: true,
                });
            }
            "armour" => {
                commands.entity(entity).insert((
                    CombatReady,
                    CombatSwing {
                        impact_at: now + f64::from(COMBAT_WINDUP_SECONDS - (*elapsed % 2.0)),
                    },
                ));
            }
            "motion" | "swim" => {
                let speed = if review.mode == "swim" {
                    if review.index == 0 {
                        1.6
                    } else {
                        0.
                    }
                } else if review.index == 0 {
                    1.6
                } else {
                    3.52
                };
                // Fixed heading and genuine translation over the 2.5-second shot.
                let velocity = Vec3::Z * speed;
                *motion = CharacterMotion::new(velocity);
                position.0 = review.origin + velocity * (*elapsed - 1.25);
            }
            _ => {}
        }
    }
}

/// Gate the screenshot sequence on the full production wardrobe, with a
/// bounded failure if a manifest/node mismatch prevents dressing forever.
pub(super) fn ready(
    mut enabled: Local<Option<bool>>,
    mut waited: Local<u32>,
    people: Query<
        (
            Has<crate::hero::HeroDressed>,
            Has<shared::components::BowEquipped>,
            Has<crate::hero::BowDressed>,
        ),
        With<CharacterReview>,
    >,
) -> bool {
    if !*enabled.get_or_insert_with(|| std::env::var("FISTFORCE_CAPTURE_CHARACTER_REVIEW").is_ok())
    {
        return true;
    }
    if !people.is_empty()
        && people
            .iter()
            .all(|(dressed, bow, bow_dressed)| dressed && (!bow || bow_dressed))
    {
        return true;
    }
    *waited += 1;
    assert!(
        *waited < 1200,
        "character review wardrobe did not become ready"
    );
    false
}
