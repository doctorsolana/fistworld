//! Direct water crossings for individually controlled heroes. Civilian routes,
//! battalions and siege retain their existing land/boat navigation contracts.
use bevy::prelude::*;
use shared::character::locomotion::{swimming_surface, SWIM_SPEED};
use shared::components::*;
use shared::terrain::WorldTerrain;

pub(super) fn surface(terrain: &WorldTerrain, point: Vec2) -> Option<f32> {
    swimming_surface(
        terrain.get_height(point.x, point.y),
        terrain.get_water_height(point.x, point.y),
    )
}

pub(super) fn speed(terrain: &WorldTerrain, point: Vec2, land_speed: f32) -> f32 {
    if surface(terrain, point).is_some() {
        SWIM_SPEED
    } else {
        land_speed
    }
}

/// Called after the ordinary ownership/availability filter, before formations.
pub(super) fn try_order(
    world: &mut World,
    units: &[Entity],
    target: Vec3,
) -> Option<(usize, String)> {
    if units.is_empty()
        || units.iter().any(|entity| {
            world.get::<CharacterKind>(*entity) != Some(&CharacterKind::Hero)
                || world.get::<MemberOfBattalion>(*entity).is_some()
                || world.get::<AboardBoat>(*entity).is_some()
        })
    {
        return None;
    }
    let terrain = world.get_resource::<WorldTerrain>()?;
    let touches_water = terrain.get_water_height(target.x, target.z).is_some()
        || units
            .iter()
            .filter_map(|e| world.get::<PlayerPosition>(*e))
            .any(|at| terrain.get_water_height(at.0.x, at.0.z).is_some());
    if !touches_water {
        return None;
    }
    let bounds = terrain.generator.active_map_bounds();
    if target.xz().cmplt(Vec2::from_array(bounds.min)).any()
        || target.xz().cmpgt(Vec2::from_array(bounds.max)).any()
    {
        return Some((0, "Destination is outside the map".into()));
    }
    for entity in units {
        let start = world.get::<PlayerPosition>(*entity)?.0.xz();
        let steps = (start.distance(target.xz()) / 0.75).ceil().max(1.0) as usize;
        let mut previous =
            surface(terrain, start).unwrap_or_else(|| terrain.get_height(start.x, start.y));
        if steps > 4096
            || !(1..=steps).all(|step| {
                let at = start.lerp(target.xz(), step as f32 / steps as f32);
                let height = surface(terrain, at).unwrap_or_else(|| terrain.get_height(at.x, at.y));
                let clear = (height - previous).abs() <= 0.85;
                previous = height;
                clear
            })
            || !super::hero::navigation_segment_clear(
                start,
                target.xz(),
                world.get_resource(),
                world.get_resource(),
                world.get_resource(),
            )
        {
            return Some((
                0,
                "Choose a clear water crossing with a gradual bank".into(),
            ));
        }
    }
    for entity in units {
        super::orders::interrupt_previous_order(world, *entity);
        world.entity_mut(*entity).insert((
            super::hero::MoveTarget(target),
            super::orders::CommandStance::Move,
        ));
    }
    Some((units.len(), "Water crossing ordered".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::hero::{step_units, MoveTarget};
    use crate::player::orders::{apply_unit_order, CommandStance};
    use shared::protocol::UnitOrder;

    fn deep_water(terrain: &WorldTerrain) -> Vec3 {
        let bounds = terrain.generator.active_map_bounds();
        for z in (bounds.min[1] as i32 + 20..bounds.max[1] as i32 - 20).step_by(32) {
            for x in (bounds.min[0] as i32 + 20..bounds.max[0] as i32 - 20).step_by(32) {
                let point = Vec2::new(x as f32, z as f32);
                if let Some(y) = surface(terrain, point) {
                    if surface(terrain, point + Vec2::X * 4.).is_some() {
                        return point.extend(y).xzy();
                    }
                }
            }
        }
        panic!("test map requires water");
    }
    #[test]
    fn owned_hero_swims_at_waterline_and_can_stop_without_sinking() {
        let terrain = WorldTerrain::default();
        let start = deep_water(&terrain);
        let mut app = App::new();
        app.insert_resource(terrain);
        app.add_systems(Update, step_units);
        let hero = app
            .world_mut()
            .spawn((
                CharacterKind::Hero,
                CommandedBy("swimmer".into()),
                PlayerPosition(start),
                PlayerRotation(0.),
                shared::region::RegionCoord::default(),
                CharacterMotion::STATIONARY,
                CharacterActivity::Idle,
                Health::default(),
            ))
            .id();
        let order = UnitOrder::move_to(vec![hero], start + Vec3::X * 3.);
        assert_eq!(
            apply_unit_order(app.world_mut(), "stranger", order.clone()).0,
            0
        );
        assert_eq!(apply_unit_order(app.world_mut(), "swimmer", order).0, 1);
        for _ in 0..90 {
            app.update();
        }
        let position = app.world().get::<PlayerPosition>(hero).unwrap().0;
        assert!((position.y - start.y).abs() < 0.01);
        assert!(position.x > start.x + 1.);
        let motion = app.world().get::<CharacterMotion>(hero).unwrap().velocity;
        assert!(motion.length() <= SWIM_SPEED + 0.02);
        apply_unit_order(
            app.world_mut(),
            "swimmer",
            UnitOrder {
                selection: shared::protocol::UnitSelection {
                    units: vec![hero],
                    battalions: vec![],
                },
                command: shared::protocol::UnitCommand::Hold,
            },
        );
        app.update();
        assert!(app.world().get::<MoveTarget>(hero).is_none());
        assert_eq!(
            app.world().get::<CommandStance>(hero),
            Some(&CommandStance::Hold)
        );
        assert!((app.world().get::<PlayerPosition>(hero).unwrap().0.y - start.y).abs() < 0.01);
    }
    #[test]
    fn civilians_and_battalions_keep_land_navigation() {
        let mut world = World::new();
        let hero = world.spawn(CharacterKind::Hero).id();
        world
            .entity_mut(hero)
            .insert(MemberOfBattalion(BattalionId(42)));
        assert!(try_order(&mut world, &[hero], Vec3::ZERO).is_none());
        world
            .entity_mut(hero)
            .remove::<MemberOfBattalion>()
            .insert(CharacterKind::Villager);
        assert!(try_order(&mut world, &[hero], Vec3::ZERO).is_none());
    }
}
