//! Compact diagnostics evaluated only for opt-in, throttled acceptance samples.
use super::*;
use crate::world::village_roads::{NavigationRouteFailed, NavigationRoutePending, TravelRoute};
use std::fmt::Debug;

fn debug<T: Debug>(value: &T) -> String {
    let text = format!("{value:?}");
    // Construction's rejected-tree list can grow. Preserve both identity and
    // the trailing phase without sending the entire list in every sample.
    if text.len() <= 1_024 {
        return text;
    }
    let mut start = 256;
    while !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = text.len() - 760;
    while !text.is_char_boundary(end) {
        end += 1;
    }
    format!("{} ... {}", &text[..start], &text[end..])
}

fn routine<T: Component + Debug>(world: &World, actor: Entity, entries: &mut Vec<String>) {
    if let Some(value) = world.get::<T>(actor) {
        entries.push(debug(value));
    }
}

pub(super) fn snapshot(world: &World, actor: Entity) -> Value {
    let mut routines = Vec::new();
    macro_rules! record { ($($kind:ty),+ $(,)?) => { $(routine::<$kind>(world, actor, &mut routines);)+ }; }
    record!(
        village::ConstructionMaterialRoutine,
        village::FarmerRoutine,
        village::FishingRoutine,
        village::LumberjackRoutine,
        village::QuarryRoutine,
        village::ProcessingRoutine,
        village::MarketCollectionRoutine,
        village::InternalDeliveryRoutine,
        village::HouseholdShoppingRoutine,
        village::MootMealRoutine,
        village::HomeRoutine,
        village::TavernWorkerRoutine,
        village::TavernVisitRoutine,
        village::TradeRouteRoutine,
        crate::world::village_roads::RoadBuilderRoutine,
        crate::world::house_upgrades::HouseUpgradeBuilderRoutine,
        crate::world::shipping::PortHaulRoutine,
        crate::world::shipping::crew::ShipCrew,
    );
    let cargo: BTreeMap<_, _> = world
        .get::<GoodsInventory>(actor)
        .into_iter()
        .flat_map(|stock| {
            Good::ALL.into_iter().filter_map(move |good| {
                let amount = stock.amount(good);
                (amount > 0).then_some((good.label(), amount))
            })
        })
        .collect();
    let route = world.get::<TravelRoute>(actor);
    json!({
        "routines": routines, "cargo": cargo,
        "queue": world.get::<village::MootQueueTicket>(actor).map(debug),
        "navigation": {
            "goal": world.get::<crate::player::hero::MoveTarget>(actor).map(|goal|goal.0.to_array()),
            "pending": world.get::<NavigationRoutePending>(actor).map(|pending|pending.goal.to_array()),
            "failed": world.get::<NavigationRouteFailed>(actor).map(|failed|failed.goal.to_array()),
            "route_goal": route.map(|route|route.goal.to_array()),
            "waypoints": route.map_or(0, |route|route.waypoints.len()),
            "next": route.map(|route|route.next),
        }
    })
}
