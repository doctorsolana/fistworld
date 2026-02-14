//! Player simulation systems.

use bevy::prelude::*;
use shared::components::{
    FlyMode, Health, Player, PlayerGrounded, PlayerJumpState, PlayerPosition, PlayerRotation,
    PlayerVelocity, PlayerWaterState,
};
use shared::physics::{step_character, WATER_SWIM_DEPTH};
use shared::player::JUMP_ANIM_MIN_SECS;
use shared::protocol::{PlayerInput, FIXED_TIMESTEP_HZ};
use shared::terrain::WorldTerrain;
use shared::vehicle::{InVehicle, VehicleState};

use crate::net::input::ClientInputs;
use crate::player::lifecycle::{is_player_alive, RespawnTimer};

/// Simulate all players.
pub fn update_players(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    inputs: Res<ClientInputs>,
    mut players: Query<(
        Entity,
        &Player,
        &Health,
        &mut PlayerPosition,
        &mut PlayerRotation,
        &mut PlayerVelocity,
        &mut PlayerGrounded,
        Option<&mut PlayerJumpState>,
        Option<&mut PlayerWaterState>,
        Option<&InVehicle>,
        Option<&RespawnTimer>,
        Option<&FlyMode>,
    )>,
    vehicles: Query<&VehicleState>,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;
    let default_input = PlayerInput::default();

    for (
        entity,
        player,
        health,
        mut position,
        mut rotation,
        mut velocity,
        mut grounded,
        jump_state,
        water_state,
        in_vehicle,
        respawn_timer,
        fly_mode,
    ) in players.iter_mut()
    {
        if !is_player_alive(health, respawn_timer) {
            velocity.0 = Vec3::ZERO;
            if fly_mode.is_some() {
                commands.entity(entity).remove::<FlyMode>();
            }
            if jump_state.is_some() {
                commands.entity(entity).remove::<PlayerJumpState>();
            }
            if water_state.is_some() {
                commands.entity(entity).remove::<PlayerWaterState>();
            }
            continue;
        }

        if let Some(in_veh) = in_vehicle {
            if let Ok(veh_state) = vehicles.get(in_veh.vehicle_entity) {
                position.0 = veh_state.position;
                rotation.0 = veh_state.heading;
                velocity.0 = Vec3::ZERO;
                grounded.on_terrain = true;
                if fly_mode.is_some() {
                    commands.entity(entity).remove::<FlyMode>();
                }
                if jump_state.is_some() {
                    commands.entity(entity).remove::<PlayerJumpState>();
                }
                if water_state.is_some() {
                    commands.entity(entity).remove::<PlayerWaterState>();
                }
                continue;
            }
        }

        let input = inputs
            .latest
            .get(&player.client_id)
            .unwrap_or(&default_input);

        let did_jump = if input.fly_mode && in_vehicle.is_none() {
            if fly_mode.is_none() {
                commands.entity(entity).insert(FlyMode);
            }
            step_character(
                input,
                &terrain,
                &mut position,
                &mut rotation,
                &mut velocity,
                &mut grounded,
                dt,
            )
        } else {
            if fly_mode.is_some() {
                commands.entity(entity).remove::<FlyMode>();
            }
            step_character(
                input,
                &terrain,
                &mut position,
                &mut rotation,
                &mut velocity,
                &mut grounded,
                dt,
            )
        };

        if did_jump {
            if let Some(mut state) = jump_state {
                state.timer = JUMP_ANIM_MIN_SECS;
            } else {
                commands.entity(entity).insert(PlayerJumpState {
                    timer: JUMP_ANIM_MIN_SECS,
                });
            }
        } else if let Some(mut state) = jump_state {
            state.timer = (state.timer - dt).max(0.0);
            if state.timer <= 0.0 {
                commands.entity(entity).remove::<PlayerJumpState>();
            }
        }

        let water_height = terrain.get_water_height(position.0.x, position.0.z);
        let had_water_state = water_state.is_some();
        if let Some(surface_y) = water_height {
            let depth = (surface_y - position.0.y).max(0.0);
            let in_water = depth > WATER_SWIM_DEPTH;
            if in_water {
                if let Some(mut state) = water_state {
                    state.in_water = true;
                    state.surface_y = surface_y;
                    state.depth = depth;
                } else {
                    commands.entity(entity).insert(PlayerWaterState {
                        in_water: true,
                        surface_y,
                        depth,
                    });
                }
            } else if had_water_state {
                commands.entity(entity).remove::<PlayerWaterState>();
            }
        } else if had_water_state {
            commands.entity(entity).remove::<PlayerWaterState>();
        }
    }
}
