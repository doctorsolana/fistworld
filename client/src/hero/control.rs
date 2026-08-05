//! Spawn intent for the hero.
//!
//! God mode: the HUD arms placement, the next left click sends
//! `DevCommand::SpawnHero`. Selecting the hero and ordering it to walk live in
//! [`crate::selection`] -- left click selects, right click commands. Everything
//! goes through the server; nothing here mutates world state directly.

use bevy::prelude::*;
use lightyear::prelude::{Connected, MessageSender};

use shared::components::{settlement_founding_refusal, Hero, HeroOutfit};
use shared::player::peer_id_to_u64;
use shared::protocol::{DevCommand, ReliableChannel, UnitMoveOrder};

use crate::camera_rts::{CursorTerrainHit, LocalPeerId};
use crate::input::InputState;
use crate::ui::hud::{GodCapability, HudMode};

/// Outfit currently chosen in the spawn panel (client-side UI state).
#[derive(Resource, Default)]
pub struct SelectedOutfit(pub HeroOutfit);

/// Whether the next terrain click places the hero (armed by the HUD button).
#[derive(Resource, Default)]
pub struct HeroSpawnArm(pub bool);

/// Whether the next terrain click drops a villager (armed by the HUD button).
///
/// Separate from [`HeroSpawnArm`] rather than one enum, because they arm from
/// different buttons and only one can be armed at a time -- arming either
/// disarms the other, which a shared bool could not express.
#[derive(Resource, Default)]
pub struct NpcSpawnArm(pub bool);

/// Whether the next terrain click founds a settlement (armed by the HUD).
#[derive(Resource, Default)]
pub struct FoundSpawnArm(pub bool);

/// True when any placement is armed, so the selection picker can stand aside.
pub fn placement_armed(hero: &HeroSpawnArm, npc: &NpcSpawnArm, found: &FoundSpawnArm) -> bool {
    hero.0 || npc.0 || found.0
}

/// True when a replicated hero owned by the local peer exists.
pub fn local_hero_exists(heroes: &Query<&Hero>, local: &LocalPeerId) -> bool {
    heroes.iter().any(|h| peer_id_to_u64(h.owner) == local.0)
}

/// The local player's hero entity, if it has replicated in.
pub fn local_hero_entity(heroes: &Query<(Entity, &Hero)>, local: &LocalPeerId) -> Option<Entity> {
    heroes
        .iter()
        .find(|(_, h)| peer_id_to_u64(h.owner) == local.0)
        .map(|(entity, _)| entity)
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
    // Grouped because Bevy caps a system at 16 parameters and these three
    // always travel together anyway: they are the same question -- what is the
    // next click for?
    arms: (
        ResMut<HeroSpawnArm>,
        ResMut<NpcSpawnArm>,
        ResMut<FoundSpawnArm>,
    ),
    local: Option<Res<LocalPeerId>>,
    heroes: Query<&Hero>,
    ui_blockers: Query<&Interaction>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
    settlements: Query<(
        &shared::components::Settlement,
        &shared::components::PlayerPosition,
    )>,
    mut notice: ResMut<crate::ui::hud::GodNotice>,
    mut dev_sender: Query<
        &mut MessageSender<DevCommand>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    let (mut arm, mut npc_arm, mut found_arm) = arms;
    // Escape cancels an armed placement. (NOT right-click: RMB-drag is the
    // camera orbit, and cancelling on it silently killed every placement
    // that involved looking around first.)
    if (arm.0 || npc_arm.0 || found_arm.0) && keyboard.just_pressed(KeyCode::Escape) {
        arm.0 = false;
        npc_arm.0 = false;
        found_arm.0 = false;
        return;
    }
    // Losing god capability or leaving god mode disarms — an invisible armed
    // state must never swallow or convert a later click.
    if (arm.0 || npc_arm.0 || found_arm.0) && (!capability.0 || *mode != HudMode::God) {
        arm.0 = false;
        npc_arm.0 = false;
        found_arm.0 = false;
    }
    if !mouse.just_pressed(MouseButton::Left) || input_state.ui_blocking() {
        return;
    }
    // A click on a HUD surface must never fall through to the world.
    if crate::ui::pointer_over_ui(&ui_blockers) {
        return;
    }
    let Some(target) = hit.0 else {
        return;
    };
    let Some(local) = local else {
        return;
    };

    let owns_hero = local_hero_exists(&heroes, &local);

    // Placement is the ONLY thing left click does here. Ordering the hero moved
    // to right click (see `crate::selection`), so a left click with nothing armed
    // is a selection click and belongs to the picker, not to this system.
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

    // Founding disarms after one placement: a settlement is a deliberate act,
    // not something to sprinkle, and the spacing rule would reject the second
    // one anyway.
    if found_arm.0 {
        // Check BEFORE sending, and say why if the answer is no.
        //
        // The server enforces these rules and always will -- it is the
        // authority. But it enforced them silently: the button disarmed, no
        // hall appeared, and the only trace was a log line on a machine the
        // player is not looking at. "I clicked and nothing happened" is the
        // worst possible answer to a deliberate act.
        //
        // The client has everything needed to predict it: it holds the terrain
        // and every settlement replicates to it. Same constants, from `shared`,
        // so the prediction cannot drift from the enforcement.
        let nearest = settlements
            .iter()
            .map(|(settlement, at)| (settlement.name.as_str(), at.0.distance(target)))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let refusal = terrain.as_ref().and_then(|terrain| {
            let centre = Vec3::new(target.x, terrain.get_height(target.x, target.z), target.z);
            settlement_founding_refusal(terrain, centre, nearest)
        });
        if let Some(reason) = refusal {
            // Stay ARMED. The player meant to found something; make them pick a
            // better spot, not press the button again.
            notice.show(reason);
            return;
        }
        if let Ok(mut sender) = dev_sender.single_mut() {
            // Empty name lets the server suggest one from the position; naming
            // properly is a UI job for when there is a text field worth using.
            sender.send::<ReliableChannel>(DevCommand::FoundSettlement {
                pos: target,
                name: String::new(),
            });
            info!("Settlement founding requested at {target:?}");
        }
        found_arm.0 = false;
        return;
    }

    // Villager placement stays armed, so a crowd can be dropped without
    // re-arming between each one. Escape or leaving god mode clears it.
    if npc_arm.0 {
        if let Ok(mut sender) = dev_sender.single_mut() {
            sender.send::<ReliableChannel>(DevCommand::SpawnNpc { pos: target });
            info!("Villager spawn requested at {target:?}");
        }
    }
}

/// Dev smoke hook: `FISTWORLD_AUTOSPAWN_HERO=1` spawns the hero at the camera
/// focus right after god capability arrives, then orders a short walk — lets
/// a headless run exercise the whole spawn->replicate->animate path without
/// UI clicks.
///
/// `FISTWORLD_AUTOSPAWN_AT="x,z"` moves the whole smoke run somewhere else.
/// Needed, not cosmetic: the camera starts at the world origin, which on
/// `big_world` is 34 metres UNDERWATER, so a run that founds at the default
/// focus is correctly refused by the water rule and tests nothing.
pub(super) fn auto_spawn_hero(
    capability: Res<GodCapability>,
    local: Option<Res<LocalPeerId>>,
    heroes: Query<(Entity, &Hero)>,
    cameras: Query<&crate::camera_rts::CommanderCamera>,
    mut dev_sender: Query<
        &mut MessageSender<DevCommand>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut move_sender: Query<
        &mut MessageSender<UnitMoveOrder>,
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
    // A bad coordinate falls back to the camera rather than to the origin: a
    // typo should behave like the flag was absent, not teleport the run into
    // the sea.
    let anchor = std::env::var("FISTWORLD_AUTOSPAWN_AT")
        .ok()
        .and_then(|spec| {
            let mut parts = spec.split(',').map(|p| p.trim().parse::<f32>().ok());
            match (parts.next().flatten(), parts.next().flatten()) {
                (Some(x), Some(z)) => Some(Vec3::new(x, 0.0, z)),
                _ => None,
            }
        })
        .unwrap_or(camera.focus);

    if *state == 0 {
        if !capability.0 {
            return;
        }
        if let Ok(mut sender) = dev_sender.single_mut() {
            sender.send::<ReliableChannel>(DevCommand::SpawnHero {
                pos: anchor,
                outfit: HeroOutfit::default(),
            });
            info!("AUTOSPAWN: hero spawn sent at {anchor:?}");
            *state = 1;
        }
        return;
    }

    // State 1: wait for our hero to replicate back, then order a walk.
    if let Some(hero_entity) = local_hero_entity(&heroes, &local) {
        if let Ok(mut sender) = move_sender.single_mut() {
            let target = anchor + Vec3::new(12.0, 0.0, 6.0);
            sender.send::<ReliableChannel>(UnitMoveOrder {
                units: vec![(hero_entity, target)],
            });
            info!("AUTOSPAWN: move order sent to {target:?}");

            // FISTWORLD_AUTOSPAWN_NPC=<n> also drops n villagers, so the god
            // SpawnNpc path is exercised headlessly instead of only by hand.
            // FISTWORLD_AUTOFOUND=1 founds a settlement at the camera focus,
            // so the whole found -> replicate -> draw path is exercised
            // headlessly rather than only by hand.
            if std::env::var("FISTWORLD_AUTOFOUND").is_ok_and(|v| v == "1") {
                if let Ok(mut dev) = dev_sender.single_mut() {
                    dev.send::<ReliableChannel>(DevCommand::FoundSettlement {
                        pos: anchor + Vec3::new(-20.0, 0.0, -20.0),
                        name: "Testholt".to_string(),
                    });
                    info!("AUTOFOUND: settlement founding sent");
                }
            }
            if let Some(count) = std::env::var("FISTWORLD_AUTOSPAWN_NPC")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
            {
                if let Ok(mut dev) = dev_sender.single_mut() {
                    for i in 0..count {
                        let pos = anchor + Vec3::new(i as f32 * 2.0 - 4.0, 0.0, -6.0);
                        dev.send::<ReliableChannel>(DevCommand::SpawnNpc { pos });
                    }
                    info!("AUTOSPAWN: {count} villager spawn(s) sent");
                }
            }
            if let Some(factor) = std::env::var("FISTWORLD_AUTOSPEED")
                .ok()
                .and_then(|value| value.parse::<f32>().ok())
                .filter(|factor| factor.is_finite() && *factor > 0.0)
            {
                if let Ok(mut dev) = dev_sender.single_mut() {
                    dev.send::<ReliableChannel>(DevCommand::SetTimeWarp(factor));
                    info!("AUTOSPAWN: requested {factor}x simulation speed");
                }
            }
            *state = 2;
        }
    }
}

/// Optional second stage for unattended visual tests.
///
/// `FISTWORLD_AUTOSPEED_AFTER="12,1"` waits twelve real seconds after entering
/// the world, then requests 1x. This lets a lab build rapidly before a recorder
/// watches animation timing at normal speed.
pub(super) fn auto_set_time_warp_after(
    time: Res<Time>,
    capability: Res<GodCapability>,
    mut dev_sender: Query<
        &mut MessageSender<DevCommand>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut state: Local<(f32, bool)>,
) {
    if state.1 || !capability.0 {
        return;
    }
    let Some((after, factor)) = std::env::var("FISTWORLD_AUTOSPEED_AFTER")
        .ok()
        .and_then(|raw| {
            let mut parts = raw.split(',').map(|part| part.trim().parse::<f32>().ok());
            match (parts.next().flatten(), parts.next().flatten()) {
                (Some(after), Some(factor))
                    if after.is_finite() && after >= 0.0 && factor.is_finite() && factor > 0.0 =>
                {
                    Some((after, factor))
                }
                _ => None,
            }
        })
    else {
        return;
    };
    state.0 += time.delta_secs();
    if state.0 < after {
        return;
    }
    if let Ok(mut dev) = dev_sender.single_mut() {
        dev.send::<ReliableChannel>(DevCommand::SetTimeWarp(factor));
        info!("AUTOSPAWN: requested {factor}x simulation speed after {after}s");
        state.1 = true;
    }
}
