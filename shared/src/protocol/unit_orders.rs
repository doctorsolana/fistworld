//! One ordered stream for all tactical unit commands. Different verbs must not
//! land in separate receivers and acquire an accidental scheduling priority.

use bevy::ecs::entity::{EntityMapper, MapEntities};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::BattalionId;

/// Bounds both individual selections and server-expanded battalions. Full
/// battalions travel as durable IDs; partial selections still name individuals.
pub const MAX_UNITS_PER_ORDER: usize = 1024;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
pub struct UnitSelection {
    pub units: Vec<Entity>,
    pub battalions: Vec<BattalionId>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Default)]
pub enum MovementMode {
    #[default]
    Move,
    AttackMove,
    Retreat,
}

/// Front-rank centre is the command target. Facing is a direction in XZ;
/// frontage is the total width of all selected blocks, including their gaps.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub struct FormationFrontage {
    pub facing: Vec2,
    pub width: f32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub enum UnitCommand {
    Move {
        target: Vec3,
        frontage: Option<FormationFrontage>,
        mode: MovementMode,
    },
    Attack {
        target: Entity,
    },
    Hold,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct UnitOrder {
    pub selection: UnitSelection,
    pub command: UnitCommand,
}

impl UnitOrder {
    pub fn move_to(units: Vec<Entity>, target: Vec3) -> Self {
        Self {
            selection: UnitSelection {
                units,
                battalions: Vec::new(),
            },
            command: UnitCommand::Move {
                target,
                frontage: None,
                mode: MovementMode::Move,
            },
        }
    }
}

impl MapEntities for UnitOrder {
    fn map_entities<M: EntityMapper>(&mut self, mapper: &mut M) {
        for unit in &mut self.selection.units {
            *unit = mapper.get_mapped(*unit);
        }
        if let UnitCommand::Attack { target } = &mut self.command {
            *target = mapper.get_mapped(*target);
        }
    }
}

/// Authoritative feedback for tactical and membership requests. A refused
/// command gives no information about entities outside the sender's authority.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ArmyOrderFeedback {
    pub accepted: u16,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mapper(u32);
    impl EntityMapper for Mapper {
        fn get_mapped(&mut self, _: Entity) -> Entity {
            self.0 += 1;
            Entity::from_raw_u32(self.0).unwrap()
        }
        fn set_mapped(&mut self, _: Entity, _: Entity) {}
    }

    #[test]
    fn every_command_roundtrips_and_maps_only_entity_references() {
        let target = Vec3::new(-64.5, 12.0, 480.0);
        for command in [
            UnitCommand::Hold,
            UnitCommand::Attack {
                target: Entity::from_raw_u32(90).unwrap(),
            },
            UnitCommand::Move {
                target,
                frontage: None,
                mode: MovementMode::Move,
            },
            UnitCommand::Move {
                target,
                frontage: Some(FormationFrontage {
                    facing: Vec2::X,
                    width: 84.0,
                }),
                mode: MovementMode::AttackMove,
            },
            UnitCommand::Move {
                target,
                frontage: None,
                mode: MovementMode::Retreat,
            },
        ] {
            let mut order = UnitOrder {
                selection: UnitSelection {
                    units: vec![Entity::from_raw_u32(50).unwrap(), Entity::PLACEHOLDER],
                    battalions: vec![BattalionId(u64::MAX)],
                },
                command,
            };
            let bytes = bincode::serialize(&order).unwrap();
            assert_eq!(bincode::deserialize::<UnitOrder>(&bytes).unwrap(), order);
            order.map_entities(&mut Mapper(0));
            assert_eq!(
                order.selection.units,
                vec![
                    Entity::from_raw_u32(1).unwrap(),
                    Entity::from_raw_u32(2).unwrap()
                ]
            );
            assert_eq!(order.selection.battalions, vec![BattalionId(u64::MAX)]);
            match command {
                UnitCommand::Attack { .. } => assert_eq!(
                    order.command,
                    UnitCommand::Attack {
                        target: Entity::from_raw_u32(3).unwrap()
                    }
                ),
                _ => assert_eq!(order.command, command),
            }
        }
    }
}
