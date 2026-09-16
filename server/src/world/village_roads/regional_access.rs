//! Bounded infrastructure certification. Each call warms at most one prop
//! chunk; callers retain their section until preparation and validation finish.

use super::*;

#[derive(Resource, Default)]
struct RegionalSurveyProps(RoutePropChunkCache, Option<(u64, u32)>);

/// `None` means preparation is still running, not an unreachable route.
pub(crate) fn regional_section_clear(
    world: &mut World,
    points: &[Vec2],
    width: f32,
) -> Option<bool> {
    regional_corridor_clear(world, points, width, true, false)
}

/// A bridge survey checks its whole footprint for obstacles, but supplies its
/// own bank/water/height certification. All trees must be absent for a deck.
pub(crate) fn regional_bridge_footprint_clear(
    world: &mut World,
    points: &[Vec2],
    width: f32,
) -> Option<bool> {
    regional_corridor_clear(world, points, width, false, true)
}

fn regional_corridor_clear(
    world: &mut World,
    points: &[Vec2],
    width: f32,
    dry: bool,
    trees_block: bool,
) -> Option<bool> {
    if points.len() < 2
        || points.len() > 512
        || !width.is_finite()
        || !(1.0..=6.0).contains(&width)
        || points.iter().any(|p| !p.is_finite())
        || points.windows(2).map(|p| p[0].distance(p[1])).sum::<f32>() > 130.0
    {
        return Some(false);
    }
    world.init_resource::<RegionalSurveyProps>();
    world.resource_scope(|world, mut cache: Mut<RegionalSurveyProps>| {
        let terrain = world.get_resource::<WorldTerrain>()?;
        let recipe = (
            terrain.generator.active_map_content_hash(),
            terrain.full_rebuild_version(),
        );
        if cache.1 != Some(recipe) {
            cache.0.chunks.clear();
            cache.1 = Some(recipe);
        }
        // Deterministic prop generation depends on the recipe, not clearances.
        // Keep the auxiliary cache finite; active sections can warm again.
        if cache.0.chunks.len() > 512 {
            cache.0.chunks.clear();
        }
        let first = points.iter().copied().reduce(Vec2::min).unwrap();
        let last = points.iter().copied().reduce(Vec2::max).unwrap();
        let props = blockers_for_route(
            terrain,
            first,
            last,
            &[],
            width * 0.5,
            world.get_resource::<StaticColliders>(),
            world.get_resource::<DerivedColliderLibrary>(),
            &mut cache.0,
            &mut false,
            trees_block,
        )?;
        let grid = world.get_resource::<SpatialObstacleGrid>();
        for edge in points.windows(2) {
            if grid.is_some_and(|g| g.segment_blocked_with_clearance(edge[0], edge[1], width * 0.5))
            {
                return Some(false);
            }
            let steps = (edge[0].distance(edge[1]) / NAVIGATION_SAMPLE_STEP)
                .ceil()
                .max(1.0) as usize;
            let mut previous = None;
            for step in 0..=steps {
                let p = edge[0].lerp(edge[1], step as f32 / steps as f32);
                if props.blocks(p) {
                    return Some(false);
                }
                if dry {
                    if !geometry::road_sample_is_dry_at_width(terrain, p, width) {
                        return Some(false);
                    }
                    let h = terrain.get_height(p.x, p.y);
                    if previous.is_some_and(|old: f32| (old - h).abs() > 0.47) {
                        return Some(false);
                    }
                    previous = Some(h);
                }
            }
        }
        Some(true)
    })
}

pub(crate) fn regional_tree_clearance(
    world: &mut World,
    points: &[Vec2],
    width: f32,
) -> RoadTreeClearancePlan {
    world.init_resource::<RegionalSurveyProps>();
    world.resource_scope(|world, mut cache: Mut<RegionalSurveyProps>| {
        let Some(terrain) = world.get_resource::<WorldTerrain>() else {
            return RoadTreeClearancePlan::default();
        };
        let mut trees = clearable_trees_intersecting_road(
            terrain,
            points,
            width,
            world.get_resource::<DerivedColliderLibrary>(),
            &mut cache.0,
        );
        if let Some(colliders) = world.get_resource::<StaticColliders>() {
            trees.retain(|tree| !colliders.road_tree_was_cleared(tree.point));
        }
        RoadTreeClearancePlan { trees }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infrastructure_ribbon_rejects_a_thin_post_between_centre_and_edge_lines() {
        let mut world = World::new();
        let terrain = WorldTerrain::default();
        let recipe = (
            terrain.generator.active_map_content_hash(),
            terrain.full_rebuild_version(),
        );
        // Isolate the live footprint check from generated props. This is a
        // clearance fixture, not proof of terrain or procedural forest quality.
        let mut props = RoutePropChunkCache::default();
        for x in -2..=2 {
            for z in -2..=2 {
                props.chunks.insert(ChunkCoord::new(x, z), Vec::new());
            }
        }
        world.insert_resource(terrain);
        world.insert_resource(RegionalSurveyProps(props, Some(recipe)));
        world.init_resource::<SpatialObstacleGrid>();
        let points = [Vec2::ZERO, Vec2::new(20., 0.)];
        assert_eq!(
            regional_bridge_footprint_clear(&mut world, &points, 3.6),
            Some(true)
        );
        world
            .resource_mut::<SpatialObstacleGrid>()
            .insert(shared::spatial::ObstacleEntry {
                center: Vec2::new(10., 0.6),
                half_extents: Vec2::new(0.2, 0.1),
                rotation: 0.,
                obstacle_type: 0,
            });
        assert!(
            [-1.8, 0., 1.8].into_iter().all(|z| !world
                .resource::<SpatialObstacleGrid>()
                .segment_blocked(points[0] + Vec2::Y * z, points[1] + Vec2::Y * z)),
            "fixture must fall between the former three sample lines"
        );
        assert_eq!(
            regional_bridge_footprint_clear(&mut world, &points, 3.6),
            Some(false)
        );
    }
}
