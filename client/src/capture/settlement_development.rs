//! Deterministic replicated readings through the ordinary compact and Places UI.
//! This proves presentation, not simulated promotion or Hall construction.

use super::{CaptureConfig, CaptureState};
use crate::ui::encyclopedia::{
    places::{KnownPlaces, SelectedPlace, SelectedPlaceEntry},
    EncyclopediaOpen, EncyclopediaTab,
};
use bevy::{
    input::InputSystems,
    prelude::*,
    ui::{UiGlobalTransform, UiSystems},
};
use shared::{components::*, economy::SettlementEconomy};

#[derive(Resource, Default)]
pub(super) struct DevelopmentStudy {
    hall: Option<Entity>,
    phase: Option<usize>,
    ready: bool,
    waiting: usize,
}

pub(super) fn install(app: &mut App) {
    if std::env::var("FISTFORCE_CAPTURE_DEVELOPMENT").as_deref() != Ok("1") {
        return;
    }
    app.init_resource::<DevelopmentStudy>()
        .add_systems(PostStartup, stage)
        .add_systems(
            PreUpdate,
            prepare.after(InputSystems).after(UiSystems::Focus),
        )
        .add_systems(Last, inspect);
}

pub(super) fn ready(study: Option<Res<DevelopmentStudy>>) -> bool {
    study.is_none_or(|study| study.ready)
}

fn stage(world: &mut World) {
    world.resource_mut::<crate::ui::hud::GodCapability>().0 = false;
    let mut at = world.resource::<CaptureConfig>().shots[0].focus;
    at.y = world
        .resource::<shared::terrain::WorldTerrain>()
        .get_height(at.x, at.z);
    let hall = world
        .spawn((
            Settlement {
                name: "Brackwater".into(),
                tier: SettlementTier::Hamlet,
                residents: 14,
                treasury: 8_600,
            },
            SettlementId(991_100),
            PlayerPosition(at),
            CivicHallLevel::for_tier(SettlementTier::Hamlet),
            SettlementDevelopment::from_seed(4, 0),
            SettlementEconomy::default(),
            shared::economy::GoodsInventory::new(600),
        ))
        .id();
    world.resource_mut::<DevelopmentStudy>().hall = Some(hall);
}

fn prepare(world: &mut World) {
    let phase = match *world.resource::<CaptureState>() {
        CaptureState::Warmup { .. } => 0,
        CaptureState::Settling { shot, .. } => shot,
        _ => return,
    };
    let card_parent = world
        .query_filtered::<&ChildOf, With<crate::ui::encyclopedia::places::PlaceDetailCard>>()
        .iter(world)
        .next()
        .map(ChildOf::parent);
    if let Some(parent) = card_parent {
        let offset = if phase == 3 || phase == 5 { 170.0 } else { 0.0 };
        world
            .entity_mut(parent)
            .insert(ScrollPosition(Vec2::new(0.0, offset)));
    }
    if world.resource::<DevelopmentStudy>().phase == Some(phase) {
        return;
    }
    let hall = world.resource::<DevelopmentStudy>().hall.unwrap();
    if world
        .resource::<KnownPlaces>()
        .find_by_id(SettlementId(991_100))
        .is_none()
    {
        return;
    }
    // Opening the book follows the production compact Expand handler.
    if phase == 1 || phase == 3 || phase == 5 {
        let Some(button) = world
            .query_filtered::<Entity, With<crate::ui::settlement_panel::InspectExpandButton>>()
            .iter(world)
            .next()
        else {
            return;
        };
        world.entity_mut(button).insert(Interaction::Pressed);
    } else if phase == 0 || phase == 2 || phase == 4 {
        world.resource_mut::<EncyclopediaOpen>().0 = false;
    }
    let town = phase == 2 || phase == 3;
    let tier = if town {
        SettlementTier::Village
    } else {
        SettlementTier::Hamlet
    };
    let mut reading = SettlementDevelopment::from_seed(4, 0);
    reading.last_progress_day = 4;
    reading.progress_days = if phase == 0 || phase == 1 || phase == 2 {
        1
    } else {
        2
    };
    reading.qualification_bits = if reading.progress_days == 1 {
        0b001
    } else {
        0b101
    };
    reading.evidence = SettlementDevelopmentEvidence {
        residents: if town { 32 } else { 14 },
        housed_residents: if town { 20 } else { 8 },
        occupied_homes: if town { 5 } else { 2 },
        operating_business_types: if phase == 2 { 1 } else { 2 },
        market_accessible: phase == 3,
        paid_trade_pennies: if phase == 3 { 1240 } else { 0 },
    };
    reading.next_gate = match phase {
        2 => SettlementProgressGate::Marketplace,
        3 | 4 => SettlementProgressGate::CivicHallMaterials,
        5 => SettlementProgressGate::CivicHallConstruction,
        _ => SettlementProgressGate::Sustaining,
    };
    if phase == 3 {
        reading.material_required = 8;
    } else if phase >= 4 {
        reading.material_staged = if phase == 4 { 7 } else { 12 };
        reading.material_required = 12;
    }
    world.get_mut::<Settlement>(hall).unwrap().tier = tier;
    world.get_mut::<Settlement>(hall).unwrap().residents = reading.evidence.residents;
    world.entity_mut(hall).insert((
        reading,
        SettlementEconomy {
            reserve_days: 0.6,
            recent_food_production: 9.0,
            recent_food_consumption: 14.0,
            unmet_food: 1,
            housing_capacity: if town { 24 } else { 8 },
            homeless_residents: if town { 12 } else { 6 },
            job_seekers: 3,
            unpaid_workers: 1,
            unrest: 24.0,
            unrest_target: 24.0,
            prosperity: 38.0,
            ..default()
        },
    ));
    world
        .resource_mut::<crate::selection::Selection>()
        .set(vec![hall]);
    *world.resource_mut::<EncyclopediaTab>() = EncyclopediaTab::Places;
    world.resource_mut::<SelectedPlace>().0 = Some(SettlementId(991_100));
    *world.resource_mut::<SelectedPlaceEntry>() = SelectedPlaceEntry::Overview;
    let mut study = world.resource_mut::<DevelopmentStudy>();
    study.phase = Some(phase);
    study.ready = false;
    study.waiting = 0;
}

fn visible_rect(world: &World, mut entity: Entity, viewport: Rect) -> Option<Rect> {
    let node = world.get::<ComputedNode>(entity)?;
    let pose = world.get::<UiGlobalTransform>(entity)?;
    let half = node.size() * 0.5;
    let mut rect = Rect {
        min: pose.transform_point2(-half),
        max: pose.transform_point2(half),
    };
    let original = rect;
    rect = rect.intersect(viewport);
    loop {
        let node = world.get::<Node>(entity)?;
        if node.display == Display::None || !world.get::<InheritedVisibility>(entity)?.get() {
            return None;
        }
        if node.overflow.x != OverflowAxis::Visible || node.overflow.y != OverflowAxis::Visible {
            let size = world.get::<ComputedNode>(entity)?.size() * 0.5;
            let pose = world.get::<UiGlobalTransform>(entity)?;
            rect = rect.intersect(Rect {
                min: pose.transform_point2(-size),
                max: pose.transform_point2(size),
            });
        }
        let Some(parent) = world.get::<ChildOf>(entity) else {
            break;
        };
        entity = parent.parent();
    }
    (rect.width() > 0.0 && rect.height() > 0.0 && original.width() > 0.0).then_some(rect)
}

fn descendant_of(world: &World, mut entity: Entity, ancestor: Entity) -> bool {
    loop {
        if entity == ancestor {
            return true;
        }
        let Some(parent) = world.get::<ChildOf>(entity) else {
            return false;
        };
        entity = parent.parent();
    }
}

fn inspect(world: &mut World) {
    let study = world.resource::<DevelopmentStudy>();
    if study.ready || study.phase.is_none() {
        return;
    }
    let phase = study.phase.unwrap();
    let hall = study.hall.unwrap();
    let config = world.resource::<CaptureConfig>();
    let viewport = Rect::from_corners(
        Vec2::ZERO,
        Vec2::new(config.resolution[0] as f32, config.resolution[1] as f32),
    );
    let book = phase % 2 == 1;
    let panel = world
        .query_filtered::<Entity, With<crate::ui::encyclopedia::EncyclopediaPanel>>()
        .iter(world)
        .find(|entity| visible_rect(world, *entity, viewport).is_some());
    let texts: Vec<_> = world.query::<(Entity, &Text)>().iter(world).filter_map(|(entity, text)| {
        if book && !panel.is_some_and(|panel| descendant_of(world, entity, panel)) { return None; }
        visible_rect(world, entity, viewport).map(|rect| serde_json::json!({"text": text.0, "min":rect.min.to_array(), "max":rect.max.to_array()}))
    }).collect();
    let contains = |value: &str| {
        texts.iter().any(|entry| {
            entry["text"]
                .as_str()
                .is_some_and(|text| text.eq_ignore_ascii_case(value))
        })
    };
    let correct = world.resource::<EncyclopediaOpen>().0 == book
        && (book == panel.is_some())
        && contains("Development")
        && contains("Living conditions")
        && contains(if phase == 2 || phase == 3 {
            "20 / 20"
        } else {
            "8 / 8"
        })
        && contains(if phase < 3 {
            "1 of last 3 · 2 needed"
        } else {
            "Approved"
        })
        && (phase != 4 || contains("7 / 12 Wood"))
        && (phase != 5 || (contains("12 / 12 Wood") && contains("Hall work underway")));
    let artwork = world
        .resource::<crate::ui::ledger::LedgerArtwork>()
        .ready(world.resource::<AssetServer>());
    let mut study = world.resource_mut::<DevelopmentStudy>();
    study.waiting += 1;
    assert!(
        study.waiting < 1800,
        "development view did not become ready in phase {phase}: {texts:?}"
    );
    if !correct || !artwork || study.waiting < 30 {
        return;
    }
    study.ready = true;
    assert!(texts
        .iter()
        .all(|entry| !entry["text"].as_str().unwrap().contains("sustained days")));
    let output = world
        .resource::<CaptureConfig>()
        .out_dir
        .join(format!("phase-{phase}.development.json"));
    let evidence = serde_json::json!({"scope":"offline replicated readings and production UI; no simulated promotion or construction", "phase":phase, "book_open":book, "development":world.get::<SettlementDevelopment>(hall), "living_conditions":world.get::<SettlementEconomy>(hall), "visible_text":texts});
    std::fs::write(output, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
}
