//! Opt-in rehearsal of real nested ledger navigation and local drafts.
//! These are offline economic facts, never a substitute for connected transaction tests.
use super::{CaptureConfig, CaptureState};
use crate::ui::{
    business_management::BusinessManagementTarget,
    company_founding::{AdjustFoundingCapital, CompanyFoundingDraft, FoundingPageOpen},
    encyclopedia::{
        companies::{self, TradeRouteEditorAction as RouteAction},
        places::{SelectedPlace, SelectedPlaceEntry},
        ClickGuard, EncyclopediaOpen, EncyclopediaPageBack, EncyclopediaPageHost,
        EncyclopediaPanel, EncyclopediaTab, TabBody,
    },
    history::{CompanyHistoryButton, HistoryPanelTarget, HistoryView},
    ledger::{LedgerArtwork, LedgerIllustration},
    market::{MarketPageTarget, PlaceMarketAction},
};
use bevy::{
    input::InputSystems,
    prelude::*,
    ui::{InteractionDisabled, UiGlobalTransform, UiSystems},
};
use shared::{components::*, economy::CompanyAccount};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Business,
    History,
    Founding,
    Market,
    Route,
}

#[derive(Resource)]
pub(super) struct Rehearsal {
    page: Page,
    staged: bool,
    pressed: Option<Entity>,
    ready_shot: Option<usize>,
    error: String,
    inspected: Option<usize>,
    initial_capital: Option<u64>,
    refresh_cash: Option<u64>,
}

pub(super) fn install(app: &mut App) {
    let Ok(mode) = std::env::var("FISTFORCE_CAPTURE_LEDGER_NESTED") else {
        return;
    };
    let page = match mode.as_str() {
        "business" => Page::Business,
        "history" => Page::History,
        "founding" => Page::Founding,
        "market" => Page::Market,
        "route" => Page::Route,
        _ => panic!("unknown nested ledger capture page: {mode}"),
    };
    app.insert_resource(Rehearsal {
        page,
        staged: false,
        pressed: None,
        ready_shot: None,
        error: "fixture not staged".into(),
        inspected: None,
        initial_capital: None,
        refresh_cash: None,
    });
    app.add_systems(Update, stage);
    app.add_systems(PreUpdate, input.after(InputSystems).after(UiSystems::Focus));
    app.add_systems(Last, inspect);
}

fn entities<T: Component>(world: &mut World) -> Vec<Entity> {
    world
        .query_filtered::<Entity, With<T>>()
        .iter(world)
        .collect()
}

fn stage(world: &mut World) {
    if world.resource::<Rehearsal>().staged {
        return;
    }
    let Some(hero) = entities::<Hero>(world).into_iter().next() else {
        return;
    };
    let Some(town) = entities::<Settlement>(world).into_iter().find(|e| {
        world
            .get::<Settlement>(*e)
            .is_some_and(|s| s.name == "Brackwater")
    }) else {
        return;
    };
    if !entities::<CompanyId>(world)
        .iter()
        .any(|e| world.get::<CompanyId>(*e) == Some(&CompanyId(501)))
    {
        return;
    }
    let owner = world.get::<Hero>(hero).unwrap().owner;
    let town_id = *world.get::<SettlementId>(town).unwrap();
    world.entity_mut(hero).insert((
        PersonId(1),
        CharacterName("Aldric".into()),
        shared::economy::Wallet::new(12_000),
    ));
    world.insert_resource(crate::camera_rts::LocalPeerId(
        shared::player::peer_id_to_u64(owner),
    ));
    // Public directory announcements corresponding to the economic fixture's sites.
    // This models completed marketplaces, without inventing a route or a transaction.
    for (id, name) in [
        (701, "Brackwater"),
        (702, "High Meadow"),
        (703, "Rivermeet"),
    ] {
        world.spawn(SettlementSummary {
            id: SettlementId(id),
            name: name.into(),
            tier: SettlementTier::Village,
            residents: 32,
            treasury: 2_000,
            prosperity: 0.6,
            reserve_days: 8.0,
            recent_food_production: 24.0,
            recent_food_consumption: 18.0,
            hungry: 0,
            housing_capacity: 40,
            homeless: 0,
            job_seekers: 0,
            unpaid_workers: 0,
            unrest: 0.0,
            unrest_change: 0.0,
            unrest_target: 0.0,
            unrest_hunger_pressure: 0.0,
            unrest_housing_pressure: 0.0,
            unrest_wage_pressure: 0.0,
            houses: 8,
            farmsteads: 2,
            fishing_huts: 1,
            lumber_huts: 1,
            windmills: 1,
            bakeries: 1,
            has_marketplace: true,
        });
    }
    world.resource_mut::<EncyclopediaOpen>().0 = true;
    *world.resource_mut::<EncyclopediaTab>() = if world.resource::<Rehearsal>().page == Page::Market
    {
        EncyclopediaTab::Places
    } else {
        EncyclopediaTab::Companies
    };
    world.resource_mut::<companies::SelectedCompany>().0 = Some(CompanyId(501));
    world.resource_mut::<SelectedPlace>().0 = Some(town_id);
    *world.resource_mut::<SelectedPlaceEntry>() = SelectedPlaceEntry::Overview;
    world.resource_mut::<Rehearsal>().staged = true;
}

fn shot(world: &World) -> Option<(usize, String)> {
    let index = match *world.resource::<CaptureState>() {
        CaptureState::Warmup { .. } => 0,
        CaptureState::Settling { shot, .. } | CaptureState::AwaitingCapture { shot, .. } => shot,
        _ => return None,
    };
    Some((
        index,
        world.resource::<CaptureConfig>().shots[index].name.clone(),
    ))
}

fn bounds(world: &World, entity: Entity) -> Option<Rect> {
    let half = world.get::<ComputedNode>(entity)?.size() * 0.5;
    let pose = world.get::<UiGlobalTransform>(entity)?;
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for corner in [
        Vec2::new(-half.x, -half.y),
        Vec2::new(half.x, -half.y),
        half,
        Vec2::new(-half.x, half.y),
    ] {
        let point = pose.transform_point2(corner);
        min = min.min(point);
        max = max.max(point);
    }
    Some(Rect { min, max })
}

fn visible(world: &World, mut entity: Entity) -> bool {
    let Some(rect) = bounds(world, entity) else {
        return false;
    };
    let size = world.resource::<CaptureConfig>().resolution;
    if rect.max.x <= 0.0
        || rect.max.y <= 0.0
        || rect.min.x >= size[0] as f32
        || rect.min.y >= size[1] as f32
    {
        return false;
    }
    loop {
        let Some(node) = world.get::<Node>(entity) else {
            return false;
        };
        if node.display == Display::None
            || world
                .get::<ComputedNode>(entity)
                .is_none_or(|c| c.size().min_element() <= 0.0)
            || world
                .get::<InheritedVisibility>(entity)
                .is_none_or(|v| !v.get())
        {
            return false;
        }
        if (node.overflow.x != OverflowAxis::Visible || node.overflow.y != OverflowAxis::Visible)
            && bounds(world, entity).is_some_and(|clip| rect.intersect(clip).is_empty())
        {
            return false;
        }
        let Some(parent) = world.get::<ChildOf>(entity) else {
            return true;
        };
        entity = parent.parent();
    }
}

fn descendant_of<T: Component>(world: &World, mut entity: Entity) -> bool {
    loop {
        if world.get::<T>(entity).is_some() {
            return true;
        }
        let Some(parent) = world.get::<ChildOf>(entity) else {
            return false;
        };
        entity = parent.parent();
    }
}

fn viewport(world: &mut World, page: Page) -> Option<Entity> {
    entities::<ScrollPosition>(world)
        .into_iter()
        .filter(|e| {
            scrolls_vertically(world, *e)
                && visible(world, *e)
                && if page == Page::Route {
                    world.get::<companies::CompanyDetailViewport>(*e).is_some()
                } else {
                    descendant_of::<EncyclopediaPageHost>(world, *e)
                }
        })
        .max_by(|a, b| {
            let area = |e| {
                world
                    .get::<ComputedNode>(e)
                    .map_or(0.0, |c| c.size().x * c.size().y)
            };
            area(*a).total_cmp(&area(*b))
        })
}

fn scrolls_vertically(world: &World, entity: Entity) -> bool {
    // ScrollPosition is required by every Bevy Node, including text and rows.
    world
        .get::<Node>(entity)
        .is_some_and(|node| node.overflow.y == OverflowAxis::Scroll)
}

fn scroll_ancestor(world: &World, mut entity: Entity) -> Option<Entity> {
    while let Some(parent) = world.get::<ChildOf>(entity) {
        entity = parent.parent();
        if scrolls_vertically(world, entity) {
            return Some(entity);
        }
    }
    None
}

fn scroll_limit(world: &World, entity: Entity) -> f32 {
    let c = world.get::<ComputedNode>(entity).unwrap();
    ((c.content_size().y - c.size().y) * c.inverse_scale_factor()).max(0.0)
}

fn page_open(world: &World, page: Page) -> bool {
    match page {
        Page::Business => world.resource::<BusinessManagementTarget>().0.is_some(),
        Page::History => world
            .resource::<HistoryPanelTarget>()
            .0
            .as_ref()
            .is_some_and(|t| t.view == HistoryView::Company(CompanyId(501))),
        Page::Founding => world.resource::<FoundingPageOpen>().0,
        Page::Market => world.resource::<MarketPageTarget>().0.is_some(),
        Page::Route => world
            .resource::<companies::TradeRouteEditorState>()
            .draft
            .is_some(),
    }
}

fn control<T: Component>(world: &mut World, matches: impl Fn(&T) -> bool) -> Option<Entity> {
    entities::<T>(world)
        .into_iter()
        .find(|e| world.get::<T>(*e).is_some_and(&matches))
}

/// Reveal a control through its actual scroll ancestor before pressing it.
fn press(world: &mut World, entity: Option<Entity>) {
    let Some(entity) = entity else {
        return;
    };
    if !world.resource::<ClickGuard>().0 || world.get::<InteractionDisabled>(entity).is_some() {
        return;
    }
    if !visible(world, entity) {
        let Some(target) = bounds(world, entity) else {
            return;
        };
        if let Some(ancestor) = scroll_ancestor(world, entity) {
            let Some(rect) = bounds(world, ancestor) else {
                return;
            };
            let inverse = world
                .get::<ComputedNode>(ancestor)
                .unwrap()
                .inverse_scale_factor();
            let limit = scroll_limit(world, ancestor);
            if let Some(mut position) = world.get_mut::<ScrollPosition>(ancestor) {
                position.y = (position.y + (target.center().y - rect.center().y) * inverse)
                    .clamp(0.0, limit);
            }
        }
        return;
    }
    if let Some(mut interaction) = world.get_mut::<Interaction>(entity) {
        *interaction = Interaction::Pressed;
    }
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.resource_mut::<Rehearsal>().pressed = Some(entity);
}

fn input(world: &mut World) {
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    if let Some(previous) = world.resource_mut::<Rehearsal>().pressed.take() {
        if let Some(mut interaction) = world.get_mut::<Interaction>(previous) {
            *interaction = Interaction::None;
        }
    }
    if !world.resource::<Rehearsal>().staged
        || !matches!(
            *world.resource::<CaptureState>(),
            CaptureState::Warmup { .. } | CaptureState::Settling { .. }
        )
    {
        return;
    }
    let Some((_, name)) = shot(world) else {
        return;
    };
    let page = world.resource::<Rehearsal>().page;
    let open = page_open(world, page);
    if name.ends_with("back") {
        if open {
            let button = if page == Page::Route {
                control::<companies::TradeRouteEditorButton>(world, |b| b.0 == RouteAction::Cancel)
            } else {
                control::<EncyclopediaPageBack>(world, |_| true)
            };
            press(world, button);
        }
        return;
    }
    if !open {
        let button = match page {
            Page::Business => control::<companies::CompanyManagementButton>(world, |b| {
                b.company == CompanyId(501)
            }),
            Page::History => {
                control::<CompanyHistoryButton>(world, |b| b.company == CompanyId(501))
            }
            Page::Founding => control::<companies::NewCompanyPageButton>(world, |_| true),
            Page::Market => control::<PlaceMarketAction>(world, |_| true),
            Page::Route => {
                control::<companies::NewTradeRouteButton>(world, |b| b.0 == CompanyId(501))
            }
        };
        press(world, button);
        return;
    }
    if page == Page::Founding
        && world.resource::<Rehearsal>().initial_capital.is_none()
        && world.resource::<CompanyFoundingDraft>().founder == Some(PersonId(1))
    {
        world.resource_mut::<Rehearsal>().initial_capital =
            Some(world.resource::<CompanyFoundingDraft>().initial_capital);
    }
    if name.ends_with("edited") {
        let button = match page {
            Page::Founding
                if world
                    .resource::<Rehearsal>()
                    .initial_capital
                    .is_some_and(|initial| {
                        world.resource::<CompanyFoundingDraft>().initial_capital == initial
                    }) =>
            {
                control::<AdjustFoundingCapital>(world, |b| b.0 == 100)
            }
            Page::Route
                if world
                    .resource::<companies::TradeRouteEditorState>()
                    .draft
                    .as_ref()
                    .is_some_and(|d| d.cargo_target == 4) =>
            {
                control::<companies::TradeRouteEditorButton>(world, |b| {
                    b.0 == RouteAction::CargoUp(1)
                })
            }
            _ => None,
        };
        if button.is_some() {
            press(world, button);
            return;
        }
        // A replicated ledger snapshot changes while the local route is edited.
        // The resulting draft must remain local, unsent, and intact.
        if page == Page::Route && world.resource::<Rehearsal>().refresh_cash.is_none() {
            if let Some(company) = entities::<CompanyId>(world)
                .into_iter()
                .find(|e| world.get::<CompanyId>(*e) == Some(&CompanyId(501)))
            {
                if let Some(mut account) = world.get_mut::<CompanyAccount>(company) {
                    account.cash += 1;
                    let cash = account.cash;
                    world.resource_mut::<Rehearsal>().refresh_cash = Some(cash);
                }
            }
        }
    }
    if let Some(viewport) = viewport(world, page) {
        let offset = if name.ends_with("lower") {
            scroll_limit(world, viewport)
        } else {
            0.0
        };
        if let Some(mut position) = world.get_mut::<ScrollPosition>(viewport) {
            position.y = offset;
        }
    }
}

fn check(world: &mut World, name: &str) -> Result<(), String> {
    macro_rules! require {
        ($condition:expr, $reason:expr) => {
            if !$condition {
                return Err($reason.into());
            }
        };
    }
    let page = world.resource::<Rehearsal>().page;
    require!(
        world.resource::<Rehearsal>().staged,
        "economic facts not staged"
    );
    require!(
        world.resource::<EncyclopediaOpen>().0,
        "Back must leave encyclopedia open"
    );
    require!(
        world
            .resource::<LedgerArtwork>()
            .ready(world.resource::<AssetServer>()),
        "book artwork loading"
    );
    let panels = entities::<EncyclopediaPanel>(world);
    require!(
        panels.len() == 1 && visible(world, panels[0]),
        "one visible book required"
    );
    let rect = bounds(world, panels[0]).unwrap();
    let size = world.resource::<CaptureConfig>().resolution;
    require!(
        rect.min.x >= -1.0
            && rect.min.y >= -1.0
            && rect.max.x <= size[0] as f32 + 1.0
            && rect.max.y <= size[1] as f32 + 1.0,
        "book must fit viewport"
    );
    let is_back = name.ends_with("back");
    require!(
        page_open(world, page) != is_back,
        "production open/back action has not completed"
    );
    let expected_tab = if page == Page::Market {
        EncyclopediaTab::Places
    } else {
        EncyclopediaTab::Companies
    };
    require!(
        *world.resource::<EncyclopediaTab>() == expected_tab,
        "nested page return tab incorrect"
    );
    let bodies = entities::<TabBody>(world)
        .into_iter()
        .filter(|e| visible(world, *e))
        .count();
    require!(
        bodies == usize::from(is_back || page == Page::Route),
        "top-level body and page host visibility disagree"
    );
    for entity in entities::<LedgerIllustration>(world) {
        if visible(world, entity) {
            require!(
                world.get::<ImageNode>(entity).is_some_and(|image| world
                    .resource::<AssetServer>()
                    .is_loaded_with_dependencies(image.image.id())),
                "visible building illustration loading"
            );
        }
    }
    if !is_back {
        if page != Page::Route {
            require!(
                entities::<EncyclopediaPageBack>(world)
                    .into_iter()
                    .any(|e| visible(world, e)),
                "nested Back control is clipped"
            );
        }
        let Some(viewport) = viewport(world, page) else {
            return Err("nested scroll viewport not laid out".into());
        };
        let scroll = world.get::<ScrollPosition>(viewport).unwrap().y;
        let expected = if name.ends_with("lower") {
            scroll_limit(world, viewport)
        } else {
            0.0
        };
        require!(
            (scroll - expected).abs() <= 1.0,
            "nested scroll has not reached requested edge"
        );
        if page == Page::Founding {
            let draft = world.resource::<CompanyFoundingDraft>();
            require!(
                draft.founder == Some(PersonId(1)) && !draft.name.is_empty() && !draft.pending,
                "founder draft must remain local and valid"
            );
            if name.ends_with("edited") {
                require!(
                    world
                        .resource::<Rehearsal>()
                        .initial_capital
                        .is_some_and(|initial| draft.initial_capital == initial + 100),
                    "capital +1 action did not update the existing draft"
                );
            }
        }
        if page == Page::Route {
            let draft = world
                .resource::<companies::TradeRouteEditorState>()
                .draft
                .as_ref()
                .unwrap();
            require!(
                draft.company == CompanyId(501)
                    && draft.route.is_none()
                    && draft.stops.len() == 2
                    && !draft.pending,
                "route draft was lost or submitted"
            );
            if name.ends_with("edited") || name.ends_with("lower") {
                require!(draft.cargo_target == 5, "local cargo edit was lost");
                let cash = world.resource::<Rehearsal>().refresh_cash;
                require!(
                    cash.is_some()
                        && world
                            .resource::<companies::CompanyDirectory>()
                            .records
                            .iter()
                            .any(|c| c.id == CompanyId(501) && Some(c.account.cash) == cash),
                    "updated ledger snapshot has not arrived in the view model"
                );
            }
        }
    }
    Ok(())
}

pub(super) fn ready(
    rehearsal: Option<Res<Rehearsal>>,
    state: Res<CaptureState>,
    config: Res<CaptureConfig>,
    mut waiting: Local<u32>,
) -> bool {
    let Some(rehearsal) = rehearsal else {
        return true;
    };
    let index = match *state {
        CaptureState::Warmup { .. } => 0,
        CaptureState::Settling { shot, .. } => shot,
        _ => return true,
    };
    if rehearsal.ready_shot == Some(index) {
        *waiting = 0;
        return true;
    }
    *waiting += 1;
    assert!(
        *waiting
            < config.shots[index]
                .readiness
                .maximum_frames
                .max(config.warmup_frames),
        "nested ledger readiness timed out: {}",
        rehearsal.error
    );
    false
}

fn inspect(world: &mut World) {
    let Some((index, name)) = shot(world) else {
        return;
    };
    let result = check(world, &name);
    let error = result.as_ref().err().cloned().unwrap_or_default();
    {
        let mut rehearsal = world.resource_mut::<Rehearsal>();
        rehearsal.ready_shot = result.is_ok().then_some(index);
        rehearsal.error = error;
    }
    if !matches!(
        *world.resource::<CaptureState>(),
        CaptureState::AwaitingCapture { .. }
    ) || world.resource::<Rehearsal>().inspected == Some(index)
    {
        return;
    }
    result.unwrap_or_else(|e| panic!("nested ledger capture failed: {e}"));
    let page = world.resource::<Rehearsal>().page;
    let scroll = viewport(world, page).map(|e| serde_json::json!({"offset":world.get::<ScrollPosition>(e).unwrap().y,"maximum":scroll_limit(world,e)}));
    let texts: Vec<String> = entities::<Text>(world)
        .into_iter()
        .filter(|e| visible(world, *e))
        .filter_map(|e| world.get::<Text>(e).map(|t| t.0.clone()))
        .collect();
    let evidence = serde_json::json!({
        "shot":name,"passed":true,"page":format!("{page:?}"),
        "input":"production button handlers, actual scroll positions",
        "fixture":"offline economic facts; no authoritative transactions submitted",
        "scroll":scroll,"visible_text":texts,
        "founding_capital":world.resource::<CompanyFoundingDraft>().initial_capital,
        "route_cargo":world.resource::<companies::TradeRouteEditorState>().draft.as_ref().map(|d|d.cargo_target),
        "refreshed_company_cash":world.resource::<Rehearsal>().refresh_cash,
    });
    let path = world
        .resource::<CaptureConfig>()
        .out_dir
        .join(format!("{name}.nested.json"));
    std::fs::write(path, serde_json::to_vec_pretty(&evidence).unwrap())
        .expect("nested capture evidence");
    world.resource_mut::<Rehearsal>().inspected = Some(index);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_skips_required_scroll_positions_on_ordinary_rows() {
        let mut world = World::new();
        let viewport = world
            .spawn(Node {
                overflow: Overflow::scroll_y(),
                ..default()
            })
            .id();
        let row = world.spawn((Node::default(), ChildOf(viewport))).id();
        let button = world.spawn((Button, Node::default(), ChildOf(row))).id();
        assert!(world.get::<ScrollPosition>(row).is_some());
        assert!(world.get::<ScrollPosition>(button).is_some());
        assert_eq!(scroll_ancestor(&world, button), Some(viewport));
        assert!(!scrolls_vertically(&world, row));
    }

    #[test]
    fn reveal_does_not_scroll_a_clipped_non_scrollable_panel() {
        let mut world = World::new();
        let panel = world
            .spawn(Node {
                overflow: Overflow::clip(),
                ..default()
            })
            .id();
        let button = world.spawn((Button, Node::default(), ChildOf(panel))).id();
        assert!(world.get::<ScrollPosition>(panel).is_some());
        assert_eq!(scroll_ancestor(&world, button), None);
    }
}
