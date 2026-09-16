//! Constant-time progress on a retained physical counter approach.

use super::{QUEUE_HEAD_PROGRESS_EPSILON, TravelRoute, Vec2, Vec3, ground_distance};
use bevy::math::Vec3Swizzles;

#[derive(Debug, Clone, Copy)]
pub(super) struct RouteProgress {
    cursor: usize,
    waypoint: Vec2,
    best_distance: f32,
}

/// Observe only the current leg and, when it advanced, its previous endpoint.
/// A replan establishes a new baseline but does not itself count as movement.
pub(super) fn observe(
    previous: &mut Option<RouteProgress>,
    position: Vec3,
    counter: Vec3,
    route: Option<&TravelRoute>,
) -> bool {
    let Some(route) = route.filter(|route| ground_distance(route.goal, counter) <= 0.1) else {
        *previous = None;
        return false;
    };
    let Some(waypoint) = route
        .waypoints
        .get(route.next)
        .map(|point| point.position.xz())
    else {
        *previous = None;
        return false;
    };
    let distance = position.xz().distance(waypoint);
    let mut next = RouteProgress {
        cursor: route.next,
        waypoint,
        best_distance: distance,
    };
    let progressed = previous.is_some_and(|prior| {
        if prior.cursor == route.next && prior.waypoint == waypoint {
            let progressed = distance + QUEUE_HEAD_PROGRESS_EPSILON < prior.best_distance;
            // Accumulate sub-epsilon movement instead of lowering the baseline
            // on every tiny step and eventually calling a slow walker stalled.
            if !progressed {
                next.best_distance = prior.best_distance;
            }
            progressed
        } else {
            // The actual mover advanced this same route beyond its last leg.
            // A different/restarted route cannot obtain free watchdog credit.
            route.next > prior.cursor
                && route
                    .waypoints
                    .get(prior.cursor)
                    .is_some_and(|point| point.position.xz() == prior.waypoint)
        }
    });
    *previous = Some(next);
    progressed
}
