//! Two production-mounted characters for socket, gait and proxy LOD inspection.
//! These are deterministic presentation inputs; the connected battle lab proves movement/combat.
use super::{CaptureConfig, CaptureFreeLook, CaptureState};
use bevy::prelude::*;
use shared::{components::*, terrain::WorldTerrain};

#[derive(Component)]
pub(super) struct ReviewMount;
#[derive(Component)]
pub(super) struct ReviewRider;

fn enabled() -> bool {
    std::env::var_os("FISTFORCE_CAPTURE_CAVALRY_VISUALS").is_some()
}

pub(super) fn stage(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    manifest: Res<crate::hero::HeroManifest>,
    mut settings: ResMut<crate::render::systems::GraphicsSettings>,
    mut done: Local<bool>,
) {
    if *done || !enabled() {
        return;
    }
    *done = true;
    settings.props_enabled = false;
    for (index, x) in [-16.8, -13.2].into_iter().enumerate() {
        let person = PersonId(910_000 + index as u64);
        let horse = 910_000 + index as u64;
        let position = PlayerPosition(Vec3::new(x, terrain.get_height(x, -15.), -15.));
        commands.spawn((
            ReviewMount,
            Horse {
                id: horse,
                rider: Some(person),
            },
            HorseAnimation {
                activity: HorseActivity::Moving(HorseGait::Trot),
                since: 0.,
            },
            position.clone(),
            PlayerRotation(std::f32::consts::PI),
            CharacterMotion::STATIONARY,
        ));
        let mut outfit = HeroOutfit::from_manifest(&manifest);
        manifest
            .apply_outfit(["soldier_padded", "soldier_mail"][index], &mut outfit)
            .expect("cavalry outfit");
        commands.spawn((
            ReviewRider,
            person,
            CharacterKind::Villager,
            CharacterName(format!("Mounted review {}", index + 1)),
            CharacterAffiliation::default(),
            CharacterActivity::Idle,
            CombatReady,
            SoldierRole::Cavalry,
            outfit,
            position,
            PlayerRotation(std::f32::consts::PI),
            CharacterMotion::STATIONARY,
            Mounted {
                horse,
                gait: HorseGait::Trot,
                phase: RidingPhase::Riding,
                since: 0.,
            },
        ));
    }
}

/// Apply the pending view before the LOD selector, so readiness can wait for
/// the requested representation instead of accidentally certifying the previous view.
pub(super) fn prepare(
    config: Res<CaptureConfig>,
    state: Res<CaptureState>,
    mut cameras: Query<&mut crate::camera_rts::CommanderCamera>,
    mut clocks: Query<&mut WorldTime>,
    mut free_look: ResMut<CaptureFreeLook>,
    mut horses: Query<&mut HorseAnimation, With<ReviewMount>>,
) {
    if !enabled() {
        return;
    }
    let index = match *state {
        CaptureState::Settling { shot, .. } | CaptureState::AwaitingCapture { shot, .. } => shot,
        _ => 0,
    };
    let Some(shot) = config.shots.get(index) else {
        return;
    };
    super::apply_shot(shot, &mut cameras, &mut clocks, &mut free_look);
    let now = clocks.iter().next().map_or(0., |c| {
        f64::from(c.day) * f64::from(c.cycle_duration()) + f64::from(c.seconds_in_cycle)
    });
    // The lighting clock is fixed; clip phase comes from the checked-in 60 Hz shot timeline.
    for mut animation in &mut horses {
        animation.since = now - index as f64 / 60.;
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn ready(
    config: Res<CaptureConfig>,
    state: Res<CaptureState>,
    horses: Query<
        (
            Entity,
            &Horse,
            &Children,
            &Visibility,
            Has<crate::animals::HorseRig>,
        ),
        With<ReviewMount>,
    >,
    riders: Query<
        (
            &PersonId,
            &Mounted,
            Option<&crate::hero::MountedVisual>,
            &Children,
            Has<crate::hero::HeroDressed>,
        ),
        With<ReviewRider>,
    >,
    scenes: Query<&Visibility, With<WorldAssetRoot>>,
    proxies: Query<&Visibility, With<Mesh3d>>,
    mut waits: Local<u32>,
    mut saved: Local<std::collections::HashSet<usize>>,
) -> bool {
    if !enabled() {
        return true;
    }
    let index = match *state {
        CaptureState::Settling { shot, .. } | CaptureState::AwaitingCapture { shot, .. } => shot,
        _ => 0,
    };
    let Some(shot) = config.shots.get(index) else {
        return true;
    };
    let wide = shot.zoom >= 800.;
    let mut rows = Vec::new();
    for (person, mounted, visual, children, dressed) in &riders {
        let Some(visual) = visual else {
            continue;
        };
        let Ok((entity, horse, horse_children, visibility, rig)) = horses.get(visual.horse) else {
            continue;
        };
        let proxy_visible = horse_children
            .iter()
            .any(|child| proxies.get(child).is_ok_and(|v| *v != Visibility::Hidden));
        let rider_visible = dressed
            && children
                .iter()
                .any(|child| scenes.get(child).is_ok_and(|v| *v != Visibility::Hidden));
        let valid = horse.id == mounted.horse
            && horse.rider == Some(*person)
            && *visibility != Visibility::Hidden
            && rider_visible
            && if wide {
                !rig && proxy_visible && !visual.ready
            } else {
                rig && visual.ready && !proxy_visible
            };
        rows.push(serde_json::json!({"person_id":person.0,"horse_id":horse.id,"horse_entity":entity.to_bits(),"rig":rig,
            "proxy_visible":proxy_visible,"rider_visible":rider_visible,"socket_ready":visual.ready,"valid":valid,
            "lower_clip":visual.lower_clip,"upper_clip":visual.upper_clip,"phase":visual.phase}));
    }
    let ready = rows.len() == 2 && rows.iter().all(|r| r["valid"] == true);
    if !ready {
        *waits += 1;
        assert!(
            *waits < 1200,
            "cavalry visual fixture never reached paired {} representation: {:?}",
            if wide { "proxy" } else { "rig" },
            rows
        );
        return false;
    }
    *waits = 0;
    if matches!(*state, CaptureState::Settling { .. })
        && (index % config.probe_every.max(1) as usize == 0 || index + 1 == config.shots.len())
        && saved.insert(index)
    {
        std::fs::create_dir_all(&config.out_dir).expect("cavalry capture directory");
        let evidence = serde_json::json!({"fixture":"production cavalry presentation","shot":shot.name,"zoom":shot.zoom,"wide":wide,"pairs":rows});
        std::fs::write(
            config.out_dir.join(format!("{}.cavalry.json", shot.name)),
            serde_json::to_vec_pretty(&evidence).unwrap(),
        )
        .expect("cavalry evidence sidecar");
    }
    true
}
