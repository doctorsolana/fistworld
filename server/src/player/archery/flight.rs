use super::*;
use crate::{collision::library::StaticColliders, world::village::mortality::PendingDeathCause};
use shared::{region::RegionCoord, terrain::WorldTerrain};
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn advance_arrows(
    mut commands: Commands,
    clock: Query<&WorldTime>,
    obstacles: Res<ArrowObstacles>,
    props: Option<Res<StaticColliders>>,
    terrain: Option<Res<WorldTerrain>>,
    mut arrows: Query<(
        Entity,
        &mut ArrowProjectile,
        &mut ArrowFlight,
        &mut RegionCoord,
    )>,
    mut people: Query<
        (
            Entity,
            &PlayerPosition,
            &mut Health,
            Option<&PlayerRotation>,
            Has<Mounted>,
        ),
        (
            Or<(With<CharacterKind>, With<Catapult>)>,
            Without<OfflineHero>,
            Without<AboardBoat>,
            Without<crate::world::village::strategic::StrategicPerson>,
        ),
    >,
    mut cells: Local<std::collections::HashMap<(i32, i32), Vec<Entity>>>,
) {
    let Some(clock) = clock.iter().next() else {
        return;
    };
    let now = seconds(clock);
    if arrows.is_empty() {
        return;
    }
    let cell = |p: Vec2| ((p.x / 4.).floor() as i32, (p.y / 4.).floor() as i32);
    for entries in cells.values_mut() {
        entries.clear();
    }
    let mut padding: f32 = 0.5;
    for (e, p, h, _, mounted) in &people {
        if !h.is_dead() {
            cells.entry(cell(p.0.xz())).or_default().push(e);
            if mounted {
                padding = collision::MOUNTED_HORIZONTAL_EXTENT;
            }
        }
    }
    cells.retain(|_, v| !v.is_empty());
    for (e, mut arrow, mut flight, mut region) in &mut arrows {
        if let Some(stop) = arrow.stopped_at {
            if now - stop > 3. {
                commands.entity(e).despawn();
            }
            continue;
        }
        let end = now.min(arrow.launched_at + f64::from(ARROW_LIFETIME));
        let elapsed = end - flight.checked_at;
        if elapsed <= 0. {
            continue;
        }
        let steps = (elapsed / 0.025).ceil().clamp(1., 320.) as usize;
        let mut hit = None;
        for i in 0..steps {
            let start_time = flight.checked_at + elapsed * i as f64 / steps as f64;
            let finish = flight.checked_at + elapsed * (i + 1) as f64 / steps as f64;
            // The mesh origin is the nock: sweep the arrowhead 0.86 m ahead.
            let a = arrow.position(start_time) + arrow.direction(start_time) * 0.86;
            let b = arrow.position(finish) + arrow.direction(finish) * 0.86;
            let mut first = obstacles.hit(a, b, props.as_deref()).map(|t| (t, None));
            if let Some(terrain) = terrain.as_deref() {
                if b.y <= terrain.get_height(b.x, b.z) {
                    let mut lo = 0.;
                    let mut hi = 1.;
                    for _ in 0..8 {
                        let t = (lo + hi) * 0.5;
                        let p = a.lerp(b, t);
                        if p.y <= terrain.get_height(p.x, p.z) {
                            hi = t;
                        } else {
                            lo = t;
                        }
                    }
                    if first.is_none_or(|(t, _)| hi < t) {
                        first = Some((hi, None));
                    }
                }
            } else if b.y <= 0. {
                let t = (a.y / (a.y - b.y)).clamp(0., 1.);
                if first.is_none_or(|(old, _)| t < old) {
                    first = Some((t, None));
                }
            }
            let lo = cell(a.xz().min(b.xz()) - Vec2::splat(padding));
            let hi = cell(a.xz().max(b.xz()) + Vec2::splat(padding));
            for x in lo.0..=hi.0 {
                for z in lo.1..=hi.1 {
                    if let Some(entries) = cells.get(&(x, z)) {
                        for &other in entries {
                            if other == flight.shooter {
                                continue;
                            }
                            let Ok((_, p, h, rotation, mounted)) = people.get(other) else {
                                continue;
                            };
                            if h.is_dead() {
                                continue;
                            }
                            let mounted_yaw = mounted.then_some(rotation.map_or(0., |r| r.0));
                            if let Some(t) = collision::body_hit(a, b, p.0, mounted_yaw) {
                                if first.is_none_or(|(old, _)| t < old) {
                                    first = Some((t, Some(other)));
                                }
                            }
                        }
                    }
                }
            }
            if let Some((fraction, victim)) = first {
                hit = Some((
                    start_time + (finish - start_time) * f64::from(fraction),
                    victim,
                ));
                break;
            }
        }
        if let Some((at, victim)) = hit {
            arrow.stopped_at = Some(at);
            if let Some(victim) = victim {
                if let Ok((_, _, mut health, _, _)) = people.get_mut(victim) {
                    let fatal = health.take_damage(32.);
                    commands
                        .entity(victim)
                        .insert(CombatReaction { at: now, fatal });
                    if fatal {
                        commands
                            .entity(victim)
                            .insert(PendingDeathCause(DeathCause::Combat));
                    }
                }
                // Body impacts use the ordinary hit reaction. Never leave an
                // arrow hovering where a moving person used to stand.
                commands.entity(e).despawn();
            }
        } else if now >= arrow.launched_at + f64::from(ARROW_LIFETIME) {
            commands.entity(e).despawn();
        }
        flight.checked_at = end;
        region.set_if_neq(RegionCoord::from_world_pos(arrow.position(end)));
    }
}
