//! Selected bow reach, grouped rather than drawn once per soldier. This is a
//! horizontal range estimate; authoritative terrain/friendly-fire tests still
//! decide whether an individual shot has a clear trajectory.

use bevy::prelude::*;
use shared::components::*;
use shared::terrain::WorldTerrain;
use std::collections::HashMap;

use super::Selection;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub(super) struct ArcherRangeGizmos;

const SEGMENTS: usize = 256;
const GROUND_LIFT: f32 = 0.18;
const REBUILD_SECONDS: f64 = 0.1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Group {
    Battalion(BattalionId),
    Detached(Entity),
}

#[derive(Default)]
struct Centre {
    sum: Vec3,
    count: usize,
}

pub(crate) struct RangeEstimate {
    group: Group,
    pub(crate) centre: Vec3,
    pub(crate) sources: usize,
    pub(crate) points: Vec<Vec3>,
    refresh_at: f64,
}

#[derive(Resource, Default)]
pub(crate) struct ArcherRangeState {
    pub(crate) ranges: Vec<RangeEstimate>,
    groups: Vec<(Group, Centre)>,
    indices: HashMap<Group, usize>,
}

#[derive(Component)]
pub(super) struct RangeCaption;

pub(super) fn install(app: &mut App) {
    app.init_resource::<ArcherRangeState>();
    app.insert_gizmo_config(
        ArcherRangeGizmos,
        GizmoConfig {
            depth_bias: -0.001,
            line: GizmoLineConfig {
                width: 2.0,
                ..default()
            },
            ..default()
        },
    );
    app.add_systems(OnEnter(crate::states::GameState::Playing), spawn_caption);
    app.add_systems(
        OnExit(crate::states::GameState::Playing),
        |mut state: ResMut<ArcherRangeState>| state.ranges.clear(),
    );
}

fn spawn_caption(mut commands: Commands) {
    commands.spawn((
        RangeCaption,
        DespawnOnExit(crate::states::GameState::Playing),
        Text::new(format!("Bow reach · {BOW_RANGE:.0} m estimate")),
        crate::ui::typography::text(13.0),
        TextColor(crate::ui::styles::PARCHMENT),
        TextShadow {
            offset: Vec2::new(0.0, 1.0),
            color: Color::srgba(0.08, 0.06, 0.04, 0.85),
        },
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            top: Val::Px(68.0),
            left: Val::Px(22.0),
            ..default()
        },
        Pickable::IGNORE,
    ));
}

fn eligible(
    role: SoldierRole,
    health: &Health,
    quiver: &Quiver,
    visibility: Option<&Visibility>,
) -> bool {
    role == SoldierRole::Archer
        && !health.is_dead()
        && quiver.arrows > 0
        && visibility != Some(&Visibility::Hidden)
}

/// Only selected entity lookups: no population or battalion scan. Scratch
/// capacity is reused, and terrain sampling is cached until the group moves
/// (at most ten rebuilds/second) or the terrain changes.
#[allow(clippy::type_complexity)]
pub(super) fn collect(
    selection: Res<Selection>,
    terrain: Option<Res<WorldTerrain>>,
    time: Res<Time>,
    camera: Query<&crate::camera_rts::CommanderCamera>,
    input: Res<crate::input::InputState>,
    people: Query<
        (
            &PlayerPosition,
            Option<&Transform>,
            &SoldierRole,
            &Health,
            &Quiver,
            Option<&MemberOfBattalion>,
            Option<&Visibility>,
        ),
        (With<BowEquipped>, Without<AboardBoat>, Without<Mounted>),
    >,
    mut state: ResMut<ArcherRangeState>,
) {
    let Some(terrain) = terrain else {
        state.ranges.clear();
        return;
    };
    if input.ui_blocking()
        || camera
            .iter()
            .next()
            .is_some_and(|camera| camera.zoom > super::ring::RING_HIDE_ZOOM)
    {
        state.ranges.clear();
        return;
    }
    let state = &mut *state;
    state.groups.clear();
    state.indices.clear();
    for entity in &selection.entities {
        let Ok((position, visual, role, health, quiver, member, visibility)) = people.get(*entity)
        else {
            continue;
        };
        if !eligible(*role, health, quiver, visibility) {
            continue;
        }
        let point = visual.map_or(position.0, |visual| visual.translation);
        if !point.is_finite() {
            continue;
        }
        let group = member.map_or(Group::Detached(*entity), |member| {
            Group::Battalion(member.0)
        });
        let index = *state.indices.entry(group).or_insert_with(|| {
            state.groups.push((group, Centre::default()));
            state.groups.len() - 1
        });
        let centre = &mut state.groups[index].1;
        centre.sum += point;
        centre.count += 1;
    }
    state.ranges.truncate(state.groups.len());
    let now = time.elapsed_secs_f64();
    for (index, (group, total)) in state.groups.iter().enumerate() {
        let centre = total.sum / total.count as f32;
        if state.ranges.len() == index {
            state.ranges.push(RangeEstimate {
                group: *group,
                centre,
                sources: total.count,
                points: Vec::with_capacity(SEGMENTS + 1),
                refresh_at: 0.0,
            });
        }
        let range = &mut state.ranges[index];
        let moved = range.centre.xz().distance_squared(centre.xz()) > 0.01;
        if range.group != *group
            || range.points.is_empty()
            || terrain.is_changed()
            || range.sources != total.count
            || selection.is_changed()
            || (moved && now >= range.refresh_at)
        {
            range.group = *group;
            range.centre = centre;
            range.points.clear();
            for index in 0..=SEGMENTS {
                let angle = index as f32 * std::f32::consts::TAU / SEGMENTS as f32;
                let x = centre.x + angle.cos() * BOW_RANGE;
                let z = centre.z + angle.sin() * BOW_RANGE;
                range
                    .points
                    .push(Vec3::new(x, terrain.get_height(x, z) + GROUND_LIFT, z));
            }
            range.refresh_at = now + REBUILD_SECONDS;
        }
        range.sources = total.count;
    }
}

pub(super) fn draw(
    state: Res<ArcherRangeState>,
    mut caption: Query<&mut Node, With<RangeCaption>>,
    mut gizmos: Gizmos<ArcherRangeGizmos>,
) {
    let display = if state.ranges.is_empty() {
        Display::None
    } else {
        Display::Flex
    };
    for mut node in &mut caption {
        if node.display != display {
            node.display = display;
        }
    }
    for range in &state.ranges {
        gizmos.linestrip(
            range.points.iter().copied(),
            Color::srgba(0.98, 0.80, 0.43, 0.62),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<Selection>()
            .init_resource::<Time>()
            .init_resource::<crate::input::InputState>()
            .init_resource::<ArcherRangeState>()
            .insert_resource(WorldTerrain::default())
            .add_systems(Update, collect);
        app
    }

    fn archer(app: &mut App, position: Vec3, group: Option<u64>) -> Entity {
        let mut entity = app.world_mut().spawn((
            PlayerPosition(position),
            SoldierRole::Archer,
            BowEquipped,
            Health::new(100.0),
            Quiver::default(),
            Visibility::Visible,
        ));
        if let Some(group) = group {
            entity.insert(MemberOfBattalion(BattalionId(group)));
        }
        entity.id()
    }

    #[test]
    fn one_estimate_per_battalion_and_no_empty_dead_sidearm_or_aboard_sources() {
        let mut app = app();
        let left = archer(&mut app, Vec3::new(-4.0, 0.0, 0.0), Some(1));
        let right = archer(&mut app, Vec3::new(4.0, 0.0, 0.0), Some(1));
        let other = archer(&mut app, Vec3::Z * 20.0, Some(2));
        let detached = archer(&mut app, Vec3::X * 20.0, None);
        let distant = archer(&mut app, Vec3::X * 200.0, None);
        app.world_mut()
            .resource_mut::<Selection>()
            .set(vec![left, right, other, detached, distant]);
        app.update();
        let state = app.world().resource::<ArcherRangeState>();
        assert_eq!(state.ranges.len(), 4);
        assert_eq!(state.ranges[0].sources, 2);
        assert_eq!(state.ranges[0].centre, Vec3::ZERO);
        assert_eq!(state.ranges[2].centre, Vec3::X * 20.0);
        assert_eq!(state.ranges[3].centre, Vec3::X * 200.0);
        for range in &state.ranges {
            assert_eq!(range.points.len(), SEGMENTS + 1);
            assert!(range
                .points
                .iter()
                .all(|point| (point.xz().distance(range.centre.xz()) - BOW_RANGE).abs() < 0.001));
            let terrain = app.world().resource::<WorldTerrain>();
            assert!(range.points.iter().all(|point| (point.y
                - terrain.get_height(point.x, point.z)
                - GROUND_LIFT)
                .abs()
                < 0.001));
        }
        app.world_mut().get_mut::<Health>(left).unwrap().current = 0.0;
        app.world_mut().get_mut::<Quiver>(right).unwrap().arrows = 0;
        app.world_mut().entity_mut(other).remove::<BowEquipped>();
        app.world_mut().entity_mut(detached).insert(AboardBoat);
        app.world_mut().entity_mut(distant).insert(Mounted {
            horse: 77,
            gait: HorseGait::Walk,
            phase: RidingPhase::Riding,
            since: 0.0,
        });
        app.update();
        assert!(app.world().resource::<ArcherRangeState>().ranges.is_empty());
    }

    #[test]
    fn deselection_hidden_units_and_modals_clear_the_range_immediately() {
        let mut app = app();
        let unit = archer(&mut app, Vec3::ZERO, None);
        app.world_mut().resource_mut::<Selection>().set(vec![unit]);
        app.update();
        assert_eq!(app.world().resource::<ArcherRangeState>().ranges.len(), 1);
        app.world_mut().resource_mut::<Selection>().clear();
        app.update();
        assert!(app.world().resource::<ArcherRangeState>().ranges.is_empty());
        app.world_mut().resource_mut::<Selection>().set(vec![unit]);
        app.world_mut()
            .get_mut::<Visibility>(unit)
            .unwrap()
            .set_if_neq(Visibility::Hidden);
        app.update();
        assert!(app.world().resource::<ArcherRangeState>().ranges.is_empty());
        *app.world_mut().get_mut::<Visibility>(unit).unwrap() = Visibility::Visible;
        app.world_mut()
            .resource_mut::<crate::input::InputState>()
            .modal_open = true;
        app.update();
        assert!(app.world().resource::<ArcherRangeState>().ranges.is_empty());
    }
}
