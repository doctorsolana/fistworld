//! Click handling + spawn intent for the hero.
//!
//! God mode: the HUD arms placement, the next terrain click sends
//! `DevCommand::SpawnHero`. Play mode: a terrain click orders the owned hero
//! to walk there. Both go through the server; nothing here mutates world
//! state directly.

use bevy::prelude::*;
use lightyear::prelude::{Connected, MessageSender};

use shared::components::{Hero, HeroOutfit};
use shared::player::peer_id_to_u64;
use shared::protocol::{DevCommand, HeroMoveTo, ReliableChannel};

use crate::camera_rts::{CursorTerrainHit, LocalPeerId};
use crate::input::InputState;
use crate::ui::hud::{GodCapability, HudMode};

/// Outfit currently chosen in the spawn panel (client-side UI state).
#[derive(Resource, Default)]
pub struct SelectedOutfit(pub HeroOutfit);

/// Whether the next terrain click places the hero (armed by the HUD button).
#[derive(Resource, Default)]
pub struct HeroSpawnArm(pub bool);

/// True when a replicated hero owned by the local peer exists.
pub fn local_hero_exists(heroes: &Query<&Hero>, local: &LocalPeerId) -> bool {
    heroes.iter().any(|h| peer_id_to_u64(h.owner) == local.0)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_world_clicks(
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    input_state: Res<InputState>,
    hit: Res<CursorTerrainHit>,
    mode: Res<HudMode>,
    capability: Res<GodCapability>,
    selected: Res<SelectedOutfit>,
    mut arm: ResMut<HeroSpawnArm>,
    local: Option<Res<LocalPeerId>>,
    heroes: Query<&Hero>,
    ui_interactions: Query<&Interaction>,
    mut dev_sender: Query<
        &mut MessageSender<DevCommand>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut move_sender: Query<
        &mut MessageSender<HeroMoveTo>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    // Escape cancels an armed placement. (NOT right-click: RMB-drag is the
    // camera orbit, and cancelling on it silently killed every placement
    // that involved looking around first.)
    if arm.0 && keyboard.just_pressed(KeyCode::Escape) {
        arm.0 = false;
        return;
    }
    // Losing god capability or leaving god mode disarms — an invisible armed
    // state must never swallow or convert a later click.
    if arm.0 && (!capability.0 || *mode != HudMode::God) {
        arm.0 = false;
    }
    if !mouse.just_pressed(MouseButton::Left) || input_state.ui_blocking() {
        return;
    }
    // A click on any HUD element must never fall through to the world.
    if ui_interactions
        .iter()
        .any(|interaction| *interaction != Interaction::None)
    {
        return;
    }
    let Some(target) = hit.0 else {
        return;
    };
    let Some(local) = local else {
        return;
    };

    let owns_hero = local_hero_exists(&heroes, &local);

    if arm.0 {
        if !owns_hero {
            if let Ok(mut sender) = dev_sender.single_mut() {
                sender.send::<ReliableChannel>(DevCommand::SpawnHero {
                    pos: target,
                    outfit: selected.0,
                });
                info!("Hero spawn requested at {target:?}");
            }
        }
        arm.0 = false;
        return;
    }

    // Play-mode click-to-move (also handy in god mode when nothing is armed).
    if owns_hero {
        if let Ok(mut sender) = move_sender.single_mut() {
            sender.send::<ReliableChannel>(HeroMoveTo { target });
        }
    }
}

/// Dev smoke hook: `FISTWORLD_AUTOSPAWN_HERO=1` spawns the hero at the camera
/// focus right after god capability arrives, then orders a short walk — lets
/// a headless run exercise the whole spawn->replicate->animate path without
/// UI clicks.
pub(super) fn auto_spawn_hero(
    capability: Res<GodCapability>,
    local: Option<Res<LocalPeerId>>,
    heroes: Query<&Hero>,
    cameras: Query<&crate::camera_rts::CommanderCamera>,
    mut dev_sender: Query<
        &mut MessageSender<DevCommand>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut move_sender: Query<
        &mut MessageSender<HeroMoveTo>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut state: Local<u8>,
) {
    if *state >= 2 || !std::env::var("FISTWORLD_AUTOSPAWN_HERO").is_ok_and(|v| v == "1") {
        return;
    }
    let Some(local) = local else { return };
    let Ok(camera) = cameras.single() else {
        return;
    };

    if *state == 0 {
        if !capability.0 {
            return;
        }
        if let Ok(mut sender) = dev_sender.single_mut() {
            sender.send::<ReliableChannel>(DevCommand::SpawnHero {
                pos: camera.focus,
                outfit: HeroOutfit::default(),
            });
            info!("AUTOSPAWN: hero spawn sent at {:?}", camera.focus);
            *state = 1;
        }
        return;
    }

    // State 1: wait for our hero to replicate back, then order a walk.
    if local_hero_exists(&heroes, &local) {
        if let Ok(mut sender) = move_sender.single_mut() {
            let target = camera.focus + Vec3::new(12.0, 0.0, 6.0);
            sender.send::<ReliableChannel>(HeroMoveTo { target });
            info!("AUTOSPAWN: move order sent to {target:?}");
            *state = 2;
        }
    }
}
