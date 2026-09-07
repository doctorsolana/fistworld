use super::*;
use shared::protocol::ArmyOrder;
/// Detached troops keep their equipment and remaining ammunition.
pub fn inherit_equipment(
    world: &mut World,
    soldier: Entity,
    role: SoldierRole,
    policy: FirePolicy,
) {
    let mut unit = world.entity_mut(soldier);
    unit.insert((role, policy));
    if role == SoldierRole::Archer {
        unit.insert_if_new(Quiver::default())
            .insert_if_new(ArcherState::default());
        if unit.get::<Quiver>().is_some_and(|q| q.arrows > 0) {
            unit.insert(BowEquipped);
        }
    } else {
        unit.remove::<(BowEquipped, BowShot)>();
    }
    if let Some(mut state) = unit.get_mut::<ArcherState>() {
        state.cancel();
    }
}
pub(crate) fn safe_to_equip(world: &mut World, members: &[Entity], account: &str) -> bool {
    let points: Vec<_> = members
        .iter()
        .filter_map(|e| world.get::<PlayerPosition>(*e).map(|p| p.0))
        .collect();
    if members.iter().any(|e| {
        world
            .get::<CharacterMotion>(*e)
            .is_some_and(|m| m.is_moving())
            || world.get::<MarchOrder>(*e).is_some()
            || world.get::<MoveTarget>(*e).is_some()
    }) {
        return false;
    }
    // Command-boundary validation, never a per-soldier tick scan. Include enemies
    // outside the close-combat index and explicit attackers of neutral people.
    !world
        .query::<(
            &PlayerPosition,
            Option<&CommandedBy>,
            Option<&super::super::combat::WarParty>,
            &Health,
        )>()
        .iter(world)
        .any(|(p, o, war, h)| {
            !h.is_dead()
                && (o.is_some_and(|o| o.0 != account) || (o.is_none() && war.is_some()))
                && points
                    .iter()
                    .any(|v| v.xz().distance_squared(p.0.xz()) < 100. * 100.)
        })
        && !members
            .iter()
            .any(|e| world.get::<AttackOrder>(*e).is_some() || world.get::<BowShot>(*e).is_some())
}
pub fn apply_equipment_order(
    world: &mut World,
    account: &str,
    order: ArmyOrder,
) -> (usize, String) {
    let battalion = match order {
        ArmyOrder::SetRole { battalion, .. }
        | ArmyOrder::SetFirePolicy { battalion, .. }
        | ArmyOrder::Rearm { battalion } => battalion,
        _ => unreachable!(),
    };
    if !world
        .get::<CommandedBy>(battalion)
        .is_some_and(|o| o.0 == account)
    {
        return (0, "Battalion unavailable".into());
    }
    let Some(id) = world.get::<Battalion>(battalion).map(|b| b.id) else {
        return (0, "Battalion unavailable".into());
    };
    let members: Vec<_> = world
        .query::<(Entity, &MemberOfBattalion, &CommandedBy, &Health)>()
        .iter(world)
        .filter(|(_, m, o, h)| m.0 == id && o.0 == account && !h.is_dead())
        .map(|(e, ..)| e)
        .collect();
    if !matches!(order, ArmyOrder::SetFirePolicy { .. }) && !safe_to_equip(world, &members, account)
    {
        return (
            0,
            "Stop at least 100 m from enemies to change equipment or rearm".into(),
        );
    }
    match order {
        ArmyOrder::SetRole { role, .. } => {
            let policy = world
                .get::<FirePolicy>(battalion)
                .copied()
                .unwrap_or_default();
            world.entity_mut(battalion).insert(role);
            for &e in &members {
                inherit_equipment(world, e, role, policy);
            }
            (members.len().max(1), format!("Equipped {}", role.label()))
        }
        ArmyOrder::SetFirePolicy { policy, .. } => {
            world.entity_mut(battalion).insert(policy);
            for &e in &members {
                world.entity_mut(e).insert(policy);
                // Pending draws cancel in shoot_bows; arrows already in flight remain real.
            }
            (members.len().max(1), policy.label().into())
        }
        ArmyOrder::Rearm { .. } => {
            for &e in &members {
                if let Some(mut q) = world.get_mut::<Quiver>(e) {
                    q.set_if_neq(Quiver::default());
                }
            }
            (members.len().max(1), "Quivers replenished".into())
        }
        _ => unreachable!(),
    }
}
