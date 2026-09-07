//! Assign objectives once at the command boundary. Per-soldier contact stays
//! local; this never turns an army click into N independent pursuit searches.
use super::*;
use crate::world::village::strategic::StrategicPerson;
use shared::{formation::FormationBlock, protocol::AttackMode};

pub(super) fn distribute_targets(
    world: &mut World,
    blocks: &[FormationBlock],
    clicked: Entity,
    mode: AttackMode,
) -> Vec<fronts::Enemy> {
    let primary = fronts::enemy_of(world, clicked);
    let mut result = vec![primary; blocks.len()];
    let fronts::Enemy::Battalion(primary_id) = primary else {
        return result;
    };
    let Some(owner) = world.get::<CommandedBy>(clicked).cloned() else {
        return result;
    };
    if mode == AttackMode::Focus || blocks.len() < 2 {
        return result;
    }
    let mut groups = BTreeMap::<BattalionId, (Vec2, usize)>::new();
    for (member, command, position, health) in world
        .query_filtered::<(&MemberOfBattalion, &CommandedBy, &PlayerPosition, &Health), (
            Without<OfflineHero>,
            Without<AboardBoat>,
            Without<StrategicPerson>,
        )>()
        .iter(world)
    {
        if command.0 == owner.0 && !health.is_dead() {
            let entry = groups.entry(member.0).or_default();
            entry.0 += position.0.xz();
            entry.1 += 1;
        }
    }
    let Some(&(sum, count)) = groups.get(&primary_id) else {
        return result;
    };
    let centre = sum / count.max(1) as f32;
    let from = blocks.iter().map(|b| b.centre.xz()).sum::<Vec2>() / blocks.len() as f32;
    let forward = (centre - from).try_normalize().unwrap_or(Vec2::Y);
    let right = Vec2::new(forward.y, -forward.x);
    // Only the line around the click. Distant reserves/other battles are not
    // invitations to send a wing off on its own.
    let candidates: Vec<_> = groups
        .into_iter()
        .filter_map(|(id, (sum, n))| {
            let at = sum / n as f32;
            let delta = at - centre;
            (delta.dot(forward).abs() <= 24.0 && delta.dot(right).abs() <= 64.0).then_some((id, at))
        })
        .collect();
    let mut assigned = vec![false; blocks.len()];
    let mut loads = vec![0usize; candidates.len()];
    // Honour the actual click with the closest friendly block first.
    if let Some(index) = blocks
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            a.centre
                .xz()
                .distance_squared(centre)
                .total_cmp(&b.centre.xz().distance_squared(centre))
                .then(a.key.cmp(&b.key))
        })
        .map(|(i, _)| i)
    {
        assigned[index] = true;
        if let Some(i) = candidates.iter().position(|(id, _)| *id == primary_id) {
            loads[i] = 1;
        }
    }
    for _ in 1..blocks.len() {
        let pair = blocks
            .iter()
            .enumerate()
            .filter(|(i, _)| !assigned[*i])
            .flat_map(|(i, block)| {
                candidates
                    .iter()
                    .enumerate()
                    .map(move |(j, (_, at))| (i, j, block.centre.xz().distance_squared(*at)))
            })
            .min_by(|a, b| {
                loads[a.1]
                    .cmp(&loads[b.1])
                    .then(a.2.total_cmp(&b.2))
                    .then(a.0.cmp(&b.0))
                    .then(a.1.cmp(&b.1))
            });
        if let Some((i, j, _)) = pair {
            assigned[i] = true;
            loads[j] += 1;
            result[i] = fronts::Enemy::Battalion(candidates[j].0);
        }
    }
    result
}
