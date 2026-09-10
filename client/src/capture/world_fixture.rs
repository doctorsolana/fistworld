//! Offline replicated world, settlement and encyclopedia fixture setup.

use super::CaptureConfig;
use super::history_fixtures::{synthetic_settlement_history, synthetic_world_history};
use super::ui_fixtures::stage_capture_companies;
use crate::states::GameState;
use bevy::prelude::*;
use shared::components::WorldTime;

/// `FISTFORCE_CAPTURE_DOOR=open` holds the offline settlement's town-hall
/// door open through the same stable building-side state used in a live game.
/// This is intentionally independent of villager AI and networking: a capture
/// made with it is a smoke test for scene instantiation, graph wiring, the
/// replicated demand consumer, and the authored glTF clip in one repeatable run.
pub(super) fn exercise_capture_door(
    mut commands: Commands,
    time: Res<Time>,
    settlements: Query<Entity, With<shared::components::Settlement>>,
    mut building_doors: Query<
        &mut shared::components::BuildingDoorDemand,
        Or<(
            With<shared::components::SettlementBuilding>,
            With<shared::components::Settlement>,
        )>,
    >,
    mut applied: Local<bool>,
) {
    // Continuous asset captures exercise both clips through the production
    // demand consumer, without substituting a capture-only animation player.
    if std::env::var("FISTFORCE_CAPTURE_DOORS").as_deref() == Ok("cycle") {
        let open = time.elapsed_secs() % 4.0 < 2.0;
        for mut door in &mut building_doors {
            if door.open != open {
                door.open = open;
            }
        }
    }
    if std::env::var("FISTFORCE_CAPTURE_DOOR").as_deref() != Ok("open") {
        return;
    }
    if *applied {
        return;
    }
    let Some(settlement) = settlements.iter().next() else {
        return;
    };
    commands
        .entity(settlement)
        .insert(shared::components::BuildingDoorDemand { open: true });
    *applied = true;
}

/// Jump straight into the world and provide the world state the server normally sends.
pub(super) fn enter_world_offline(
    mut commands: Commands,
    mut next_state: ResMut<NextState<GameState>>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
) {
    // No connection, no name entry — the map comes off disk.
    next_state.set(
        match std::env::var("FISTFORCE_CAPTURE_FRONTEND").as_deref() {
            Ok("menu") => GameState::MainMenu,
            Ok("name") => GameState::Connected,
            _ => GameState::Playing,
        },
    );

    // Stand in for the replicated WorldTime, otherwise day/night never advances past
    // "waiting for server" and every shot is unlit.
    commands.spawn(WorldTime {
        seconds_in_cycle: 0.0,
        day_duration: 600.0,
        night_duration: 300.0,
        ocean_seconds: 0.0,
        day: 0,
    });
    // Same stand-in for CloudSeed — the cloud plane waits for it. Fixed seed so
    // shots are reproducible; FISTFORCE_CAPTURE_CLOUDS=clear|cloudy overrides
    // the weather roll for guaranteed cloud coverage in verification shots.
    commands.spawn(shared::components::CloudSeed { seed: 7 });
    if let Ok(forced) = std::env::var("FISTFORCE_CAPTURE_CLOUDS") {
        use crate::render::systems::{CloudCover, CloudCoverMode, CloudCoverOverride};
        let mode = match forced.as_str() {
            "cloudy" => Some(CloudCoverMode::Cloudy),
            "clear" => Some(CloudCoverMode::Clear),
            "storm" => Some(CloudCoverMode::Storm),
            _ => None,
        };
        if let Some(mode) = mode {
            commands.insert_resource(CloudCoverOverride { mode });
            // Snap: a capture's few warmup seconds can't ride the ~90s lerp.
            commands.insert_resource(CloudCover::snapped(mode));
        }
    }

    // FISTFORCE_CAPTURE_HUD=play|god draws the persistent HUD, which is
    // otherwise suppressed so world shots stay clean. `god` also grants the god
    // capability and switches mode, so the god plate is visible.
    if std::env::var("FISTFORCE_CAPTURE_HUD").is_ok_and(|mode| mode == "god") {
        commands.insert_resource(crate::ui::hud::GodCapability(true));
        commands.insert_resource(crate::ui::hud::HudMode::God);
    }

    // FISTFORCE_CAPTURE_DEBUG_MENU=god|access opens the real J menu without
    // synthesizing keyboard input. Keeping this as a first-class capture target
    // prevents developer-only screens from escaping the ordinary UI visual audit.
    if let Ok(mode) = std::env::var("FISTFORCE_CAPTURE_DEBUG_MENU") {
        let god_access = mode.trim().eq_ignore_ascii_case("god");
        commands.insert_resource(crate::ui::debug_time_menu::DebugTimeMenuOpen(true));
        commands.insert_resource(crate::ui::hud::GodCapability(god_access));
        if god_access {
            commands.insert_resource(crate::ui::hud::HudMode::God);
        }
    }

    // FISTFORCE_CAPTURE_ROADS=compare stages the same completed main road
    // before and after its real Dirt -> Stone upgrade. It deliberately spawns
    // only replicated road data: the ordinary terrain compositor must produce
    // the image, so this remains a regression fixture for the shipping path.
    if std::env::var("FISTFORCE_CAPTURE_ROADS").is_ok_and(|value| value == "compare") {
        commands.queue(|world: &mut World| {
            let focus = world
                .get_resource::<CaptureConfig>()
                .and_then(|config| config.shots.first().map(|shot| shot.focus))
                .unwrap_or_default();
            let relative = [
                Vec2::new(-34.0, 0.0),
                Vec2::new(-12.0, -2.0),
                Vec2::new(8.0, 1.0),
                Vec2::new(34.0, 0.0),
            ];
            let points_at = |z: f32| {
                relative
                    .iter()
                    .map(|point| Vec2::new(focus.x + point.x, focus.z + point.y + z))
                    .collect::<Vec<_>>()
            };
            world.spawn(shared::components::VillageRoad {
                settlement: "Capture Roads".into(),
                builder: "Capture Road Steward".into(),
                points: points_at(-7.0),
                built_through: relative.len() as u16,
                width: 2.6,
                reserved_width: shared::components::RoadClass::Main.initial_reserved_width(),
                surface: shared::components::RoadSurface::Dirt,
                class: shared::components::RoadClass::Main,
                stone_committed: 0,
            });
            let mut stone = shared::components::VillageRoad {
                settlement: "Capture Roads".into(),
                builder: "Capture Road Steward".into(),
                points: points_at(7.0),
                built_through: relative.len() as u16,
                width: 4.0,
                reserved_width: shared::components::RoadClass::Main.initial_reserved_width(),
                surface: shared::components::RoadSurface::Stone,
                class: shared::components::RoadClass::Main,
                stone_committed: 0,
            };
            stone.stone_committed = stone.stone_required();
            world.spawn(stone);
        });
    }

    // FISTFORCE_CAPTURE_PAUSE=main|graphics|controls photographs the real ESC
    // menu and its expanded settings wells without needing keyboard input.
    if let Ok(panel) = std::env::var("FISTFORCE_CAPTURE_PAUSE") {
        crate::ui::pause_menu::open_for_capture(&mut commands, panel.trim());
    }

    // FISTFORCE_CAPTURE_SETTLEMENT=1 founds a settlement at the shot's focus so
    // the moot hall can be photographed without a server.
    // "village" additionally populates the first one; "coast" stages the
    // deterministic Village Lab hut/pier pair for shoreline inspection; and
    // "industries" frames the authored founding production buildings closely;
    // "bakery" isolates a staffed bakery for chimney, lighting and stock-art QA;
    // "market" and "market_paved" isolate the two identically sized square levels;
    // "storage_hall", "lumberjack" and "windmill" isolate their art, door wiring and night lights.
    if std::env::var("FISTFORCE_CAPTURE_SETTLEMENT").is_ok_and(|v| {
        matches!(
            v.as_str(),
            "1" | "village"
                | "coast"
                | "industries"
                | "bakery" | "tavern"
                | "market"
                | "market_paved"
                | "storage_hall"
                | "lumberjack"
                | "windmill"
        )
    }) {
        commands.queue(|world: &mut World| {
            // The FIRST SHOT's focus, not the camera's: this runs in Startup,
            // before `apply_shot` has moved the camera, so reading the camera
            // here plants the settlement at the origin and photographs empty
            // ground two kilometres away from it.
            let focus = world
                .get_resource::<CaptureConfig>()
                .and_then(|c| c.shots.first().map(|s| s.focus))
                .unwrap_or_default();
            let mode = std::env::var("FISTFORCE_CAPTURE_SETTLEMENT").unwrap_or_default();
            if matches!(mode.as_str(), "market" | "market_paved" | "storage_hall" | "lumberjack" | "windmill") {
                // Reproduce the authoritative construction earthwork in this
                // network-free visual fixture. This makes the capture useful
                // for spotting terrain triangles through the 12 m ground slab,
                // not merely for checking the GLB in isolation.
                if let Some(mut terrain) =
                    world.get_resource_mut::<shared::terrain::WorldTerrain>()
                {
                    let kind = if mode == "storage_hall" {
                        shared::components::SettlementBuildingKind::StorageHall
                    } else if mode == "windmill" {
                        shared::components::SettlementBuildingKind::Windmill
                    } else if mode == "lumberjack" {
                        shared::components::SettlementBuildingKind::LumberjackHut
                    } else {
                        shared::components::SettlementBuildingKind::Market
                    };
                    let def = kind.art().definition();
                    let target = terrain.get_height(focus.x, focus.z);
                    terrain.apply_flatten_rect(
                        Vec3::new(focus.x, target, focus.z),
                        def.terrain_flat_half_extents(),
                        0.0,
                        def.terrain_blend_width(),
                    );
                }
            }
            let settlement_focus = match mode.as_str() {
                "coast" => {
                    // The requested focus is the hut. This is its deterministic
                    // offset from the lab hall selected on village_lab seed 3.
                    focus - Vec3::new(53.94803, 0.0, -41.39578)
                }
                // Centre the authored production cluster rather than its Hall.
                // This keeps close asset-validation shots reusable as the Hall
                // ladder grows substantially taller than founding industries.
                "industries" | "bakery" | "tavern" | "market" | "market_paved" | "storage_hall" | "lumberjack" | "windmill" => {
                    focus + Vec3::new(0.0, 0.0, 140.0)
                }
                _ if std::env::var("FISTFORCE_CAPTURE_PERMIT_PLACEMENT").is_ok() => {
                    focus + Vec3::new(0.0, 0.0, -50.0)
                }
                _ => focus,
            };
            let ground = world
                .get_resource::<shared::terrain::WorldTerrain>()
                .map(|t| t.get_height(settlement_focus.x, settlement_focus.z))
                .unwrap_or(settlement_focus.y);
            use shared::components::SettlementTier as T;
            // A spread of rungs, so the list's ordering and the detail pane's
            // per-rung wording can both be photographed.
            for (i, (name, tier, offset)) in [
                ("Brackwater", T::Town, Vec3::new(0.0, 0.0, 0.0)),
                ("Ashfell", T::Hamlet, Vec3::new(-1400.0, 0.0, -1900.0)),
                ("Millhollow", T::Village, Vec3::new(900.0, 0.0, 1500.0)),
                ("Coldbarrow", T::Hamlet, Vec3::new(1800.0, 0.0, -600.0)),
            ]
            .into_iter()
            .enumerate()
            {
                let at = settlement_focus + offset;
                let y = if i == 0 { ground } else { at.y };
                let hall = world
                    .spawn((
                        shared::components::Settlement {
                            name: name.to_string(),
                            tier,
                            residents: if i == 0 {
                                // A UI-only override for reviewing population-dependent
                                // civic capacity; it does not stage a simulated town.
                                std::env::var("FISTFORCE_CAPTURE_CIVIC_POPULATION")
                                    .ok().and_then(|value| value.parse().ok()).unwrap_or(0)
                            } else { (i as u32) * 3 },
                            treasury: 0,
                        },
                        shared::components::SettlementId(i as u64 + 1),
                        shared::components::PlayerPosition(Vec3::new(at.x, y, at.z)),
                    ))
                    .id();
                if i == 0 && mode == "village" {
                    let mut store = shared::economy::GoodsInventory::new_partitioned(
                        shared::economy::capacity::HALL,
                    );
                    store.add(shared::economy::Good::Food, 9);
                    store.add(shared::economy::Good::Wheat, 18);
                    store.add(shared::economy::Good::Wood, 7);
                    store.add(shared::economy::Good::Stone, 2);
                    let mut market = shared::economy::MootMarket::founding();
                    let treasury = shared::economy::MarketSeller::Treasury(
                        shared::components::SettlementId(1),
                    );
                    market.consign(treasury, shared::economy::Good::Food, 6, 105);
                    market.consign(treasury, shared::economy::Good::Wheat, 12, 86);
                    market.consign(treasury, shared::economy::Good::Wood, 5, 64);
                    market.consign(treasury, shared::economy::Good::Stone, 2, 285);
                    market.refresh_all(&store);
                    world.entity_mut(hall).insert((
                        store,
                        market,
                        shared::components::MootAdministration {
                            lead_steward: Some(shared::names::person_name(7_002)),
                            roadless_buildings: 1,
                            disconnected_buildings: 0,
                            last_road_audit_day: 12,
                            ..default()
                        },
                        shared::components::SettlementPolicies::poor_relief(),
                        shared::components::SettlementOpportunityBoard {
                            opportunities: vec![
                                shared::components::PermitMarketOpportunity {
                                    kind: shared::components::SettlementBuildingKind::House,
                                    score: 94,
                                    subsidized: true,
                                    requires_independent_owner: false,
                                },
                                shared::components::PermitMarketOpportunity {
                                    kind: shared::components::SettlementBuildingKind::Farmstead,
                                    score: 78,
                                    subsidized: true,
                                    requires_independent_owner: false,
                                },
                                shared::components::PermitMarketOpportunity {
                                    kind: shared::components::SettlementBuildingKind::Windmill,
                                    score: 46,
                                    subsidized: false,
                                    requires_independent_owner: true,
                                },
                                shared::components::PermitMarketOpportunity {
                                    kind: shared::components::SettlementBuildingKind::Bakery,
                                    score: 31,
                                    subsidized: false,
                                    requires_independent_owner: false,
                                },
                            ],
                        },
                        shared::components::SettlementPropertyBoard {
                            listings: vec![
                                shared::components::PropertyMarketListing {
                                    kind: shared::components::SettlementBuildingKind::Bakery,
                                    stage: shared::components::PropertyListingStage::CompletedBusiness,
                                    asking_price: 425,
                                    listed_day: 0,
                                    reason: shared::economy::BusinessSaleReason::Insolvent,
                                    position: settlement_focus + Vec3::new(46.0, 0.0, 34.0),
                                },
                                shared::components::PropertyMarketListing {
                                    kind: shared::components::SettlementBuildingKind::Windmill,
                                    stage: shared::components::PropertyListingStage::UnfinishedWorksite,
                                    asking_price: 250,
                                    listed_day: 0,
                                    reason: shared::economy::BusinessSaleReason::OwnerDied,
                                    position: settlement_focus + Vec3::new(-38.0, 0.0, 26.0),
                                },
                            ],
                        },
                        shared::economy::SettlementEconomy {
                            edible_stock: 27,
                            reserve_days: 4.5,
                            recent_food_production: 3.7,
                            recent_food_consumption: 3.0,
                            unmet_food: 0,
                            observed_days: 8,
                            food_secure_days: 5,
                            reserve_prosperity: 22.0,
                            production_prosperity: 20.0,
                            housing_prosperity: 18.0,
                            employment_prosperity: 18.0,
                            hunger_penalty: 0.0,
                            prosperity: 78.0,
                            private_job_positions: 8,
                            private_filled_jobs: 6,
                            private_vacant_jobs: 2,
                            civic_job_positions: 3,
                            civic_filled_jobs: 2,
                            civic_vacant_jobs: 1,
                            job_seekers: 1,
                            best_open_private_wage: 120,
                            housing_capacity: 8,
                            homeless_residents: 0,
                            unpaid_workers: 0,
                            unrest: 8.0,
                            unrest_target: 0.0,
                            unrest_change: -5.0,
                            unrest_hunger_pressure: 0.0,
                            unrest_housing_pressure: 0.0,
                            unrest_wage_pressure: 0.0,
                        },
                    ));
                }
            }

            // FISTFORCE_CAPTURE_SETTLEMENT=village also populates the FIRST
            // settlement: residents on record, buildings standing, one going
            // up. These are offline stand-ins for what the server's autonomy
            // produces, so the settlement panel can be photographed without
            // waiting out a live village.
            if std::env::var("FISTFORCE_CAPTURE_SETTLEMENT")
                .is_ok_and(|v| {
                    matches!(
                        v.as_str(),
                        "village" | "industries" | "bakery" | "tavern" | "market" | "market_paved" | "storage_hall" | "lumberjack" | "windmill"
                    )
                })
            {
                use shared::components::SettlementBuildingKind as K;
                let people: Vec<String> = (0..3)
                    .map(|i| shared::names::person_name(7_000 + i))
                    .collect();
                for (index, name) in people.iter().enumerate() {
                    world.spawn((
                        shared::components::CharacterName(name.clone()),
                        shared::components::CharacterKind::Villager,
                        shared::components::Residence("Brackwater".to_string()),
                        shared::components::Occupation(Some(
                            ["Farmer", "Lumberjack", "Road Steward"][index].to_string(),
                        )),
                        shared::economy::Wallet::new((650 + index as u64 * 275) * 100),
                        shared::components::Nutrition {
                            last_meal_day: Some(12),
                            consecutive_missed_meals: if index == 1 { 1 } else { 0 },
                            total_meals: 12,
                            total_missed_meals: if index == 1 { 1 } else { 0 },
                        },
                        shared::components::CharacterActivity::Indoors,
                    ));
                }
                let kinds: &[K] = if mode == "storage_hall" {
                    &[K::StorageHall]
                } else if mode == "windmill" {
                    &[K::Windmill]
                } else if mode == "lumberjack" {
                    &[K::LumberjackHut]
                } else if mode == "tavern" {
                    &[K::Tavern]
                } else if mode == "bakery" {
                    &[K::Bakery]
                } else if matches!(mode.as_str(), "market" | "market_paved") {
                    &[K::Market]
                } else {
                    &[K::Farmstead, K::LumberjackHut, K::Windmill, K::Bakery]
                };
                for (index, kind) in kinds.iter().copied().enumerate() {
                    let at = if matches!(mode.as_str(), "bakery" | "tavern" | "market" | "market_paved" | "storage_hall" | "lumberjack" | "windmill") {
                        focus
                    } else if mode == "industries" {
                        // One authored comparison line: equal frontage,
                        // spacing and rotation make scale/anchor mistakes
                        // obvious in a single frame.
                        focus + Vec3::new(-27.0 + index as f32 * 18.0, 0.0, 0.0)
                    } else {
                        let column = index % 2;
                        let row = index / 2;
                        settlement_focus
                            + Vec3::new(30.0 + column as f32 * 16.0, 0.0, 18.0 + row as f32 * 17.0)
                    };
                    let ground = world
                        .get_resource::<shared::terrain::WorldTerrain>()
                        .map(|t| t.get_height(at.x, at.z))
                        .unwrap_or(at.y);
                    if matches!(kind, K::Bakery | K::Tavern) {
                        // Art inspection uses the same level plot as real placement.
                        let def = kind.art().definition();
                        let centre = def.world_footprint_center(Vec3::new(at.x, ground, at.z), 0.0);
                        if let Some(mut terrain) = world.get_resource_mut::<shared::terrain::WorldTerrain>() {
                            terrain.apply_flatten_rect(
                                Vec3::new(centre.x, ground, centre.y),
                                def.terrain_flat_half_extents(), 0.0, def.terrain_blend_width(),
                            );
                        }
                    }
                    let mut store =
                        shared::economy::GoodsInventory::new(kind.storage_bulk_capacity());
                    match kind {
                        K::Farmstead => {
                            store.add(shared::economy::Good::Wheat, 11);
                        }
                        K::LumberjackHut => {
                            store.add(shared::economy::Good::Wood, 6);
                        }
                        K::Windmill => {
                            store.add(shared::economy::Good::Wheat, 8);
                            store.add(shared::economy::Good::Flour, 3);
                        }
                        K::Tavern => { store.add(shared::economy::Good::Bread, 24); }
                        K::Bakery => {
                            store.add(shared::economy::Good::Flour, 6);
                            store.add(shared::economy::Good::Bread, 160);
                        }
                        K::Market | K::StorageHall => {
                            store.add(shared::economy::Good::Food, 18);
                            store.add(shared::economy::Good::Wood, 10);
                        }
                        _ => unreachable!(),
                    }
                    let operator = &people[index % people.len()];
                    let mut business = shared::economy::BusinessAccount::with_capital(2_000);
                    business.record_sale(11, 800 + index as u64 * 125, 40, 4);
                    business.incur_wages(11, 200);
                    business.settle_wage_claim(200);
                    business.roll_to_day(12);
                    let building_entity = world
                        .spawn((
                        shared::components::SettlementBuilding {
                            kind,
                            settlement: "Brackwater".to_string(),
                            owner: Some(operator.clone()),
                            // Stand-ins, like the resident names above. The
                            // client has no BiomeField truth to sample and must
                            // not invent one -- these numbers exist so the panel
                            // has something to lay out, nothing more.
                            quality: if index == 0 { 0.82 } else { 0.41 },
                            workers: vec![operator.clone()],
                        },
                        shared::components::PlayerPosition(Vec3::new(at.x, ground, at.z)),
                        shared::components::PlayerRotation(0.0),
                        shared::components::BuildingId(100 + index as u64),
                        store,
                        business,
                        shared::economy::BusinessCondition::default(),
                        shared::economy::BusinessManagementPolicy::default(),
                        shared::economy::BusinessProcurementPolicy::default(),
                        shared::economy::BusinessSalePolicy::for_good(match kind {
                            K::Farmstead => shared::economy::Good::Wheat,
                            K::LumberjackHut => shared::economy::Good::Wood,
                            K::Windmill => shared::economy::Good::Flour,
                            K::Bakery | K::Tavern => shared::economy::Good::Bread,
                            K::Market | K::StorageHall => shared::economy::Good::Wood,
                            _ => unreachable!(),
                        }),
                        shared::economy::BusinessWagePolicy::default(),
                        shared::components::BuildingDoorDemand {
                            open: std::env::var("FISTFORCE_CAPTURE_DOORS")
                                .is_ok_and(|value| value == "open"),
                        },
                    ))
                        .id();
                    if kind == K::Bakery {
                        // Visual fixture equivalent of a staffed, supplied
                        // bakery: exercise the same replicated transition that
                        // live server production drives.
                        world.entity_mut(building_entity).insert(
                            shared::components::WorkplaceOperation { active_workers: 1 },
                        );
                    }
                    if kind == K::Market {
                        world.entity_mut(building_entity).insert(
                            if mode == "market_paved" {
                                shared::components::MarketLevel::Paved
                            } else {
                                shared::components::MarketLevel::Earthen
                            },
                        );
                    }
                }
                if mode == "village" {
                    let house_at = focus + Vec3::new(15.0, 0.0, -12.0);
                    let house_ground = world
                        .get_resource::<shared::terrain::WorldTerrain>()
                        .map(|t| t.get_height(house_at.x, house_at.z))
                        .unwrap_or(house_at.y);
                    world.spawn((
                        shared::components::SettlementBuilding {
                            kind: K::House,
                            settlement: "Brackwater".to_string(),
                            owner: Some(people[2].clone()),
                            quality: 0.5,
                            workers: Vec::new(),
                        },
                        shared::components::Household {
                            residents: people.clone(),
                            ..default()
                        },
                        shared::components::PlayerPosition(Vec3::new(
                            house_at.x,
                            house_ground,
                            house_at.z,
                        )),
                        // Broadside to the default capture camera so the authored
                        // pane, rather than only its edge, is available for visual
                        // day/night comparison.
                        shared::components::PlayerRotation(1.02),
                    ));
                    // A site needs a POSITION as well as its record -- the raise
                    // visual is placed from it, and without one the frame has
                    // nowhere to come out of.
                    let site_at = focus + Vec3::new(-14.0, 0.0, 10.0);
                    let site_ground = world
                        .get_resource::<shared::terrain::WorldTerrain>()
                        .map(|t| t.get_height(site_at.x, site_at.z))
                        .unwrap_or(site_at.y);
                    world.spawn((
                        shared::components::ConstructionSite {
                            kind: K::House,
                            settlement: "Brackwater".to_string(),
                            // Mid-raise, so a screenshot catches the frame partly
                            // out of the ground rather than an empty plot.
                            raising: true,
                            stand: shared::components::builder_stand_position(
                                site_at,
                                0.9,
                                K::House.art().definition().footprint.y,
                            ),
                            // Deliberately NOT zero: a rotated site is the case
                            // where a mismatch between the rising frame and the
                            // finished building would show.
                            rotation: 0.9,
                        },
                        {
                            let mut materials = shared::economy::GoodsInventory::new(
                                K::House.construction_storage_bulk(),
                            );
                            materials.add(
                                shared::economy::Good::Wood,
                                K::House.construction_wood_required(),
                            );
                            materials
                        },
                        shared::components::PlayerPosition(Vec3::new(
                            site_at.x,
                            site_ground,
                            site_at.z,
                        )),
                    ));
                    // A completed connector through the staged plot also makes
                    // this fixture useful for ground-cover exclusion captures.
                    world.spawn(shared::components::VillageRoad {
                        settlement: "Brackwater".to_string(),
                        builder: "Capture Road Steward".to_string(),
                        points: vec![
                            Vec2::new(focus.x - 35.0, focus.z - 3.0),
                            Vec2::new(focus.x + 35.0, focus.z - 3.0),
                        ],
                        built_through: 2,
                        width: 3.0,
                        reserved_width: 4.0,
                        surface: default(),
                        class: default(),
                        stone_committed: 0,
                    });
                }
                if let Some(mut settlement) = world
                    .query::<&mut shared::components::Settlement>()
                    .iter_mut(world)
                    .find(|s| s.name == "Brackwater")
                {
                    settlement.residents = std::env::var("FISTFORCE_CAPTURE_CIVIC_POPULATION")
                        .ok().and_then(|value| value.parse().ok()).unwrap_or(people.len() as u32);
                    settlement.treasury = 2_750;
                }
                if std::env::var("FISTFORCE_CAPTURE_TRADE").is_ok_and(|value| value == "1") {
                    let hall = world
                        .query_filtered::<Entity, With<shared::components::Settlement>>()
                        .iter(world)
                        .find(|entity| {
                            world
                                .get::<shared::components::Settlement>(*entity)
                                .is_some_and(|settlement| settlement.name == "Brackwater")
                        });
                    if let Some(hall) = hall {
                        let brackwater_id = world
                            .get::<shared::components::SettlementId>(hall)
                            .copied()
                            .unwrap_or_default();
                        world.insert_resource(crate::ui::market::MarketPageTarget(Some(
                            crate::ui::market::MarketPage {
                                settlement: hall,
                                place: "Brackwater".into(),
                                place_id: brackwater_id,
                            },
                        )));
                        world.resource_mut::<crate::ui::encyclopedia::EncyclopediaOpen>().0 = true;
                        *world.resource_mut::<crate::ui::encyclopedia::EncyclopediaTab>() =
                            crate::ui::encyclopedia::EncyclopediaTab::Places;
                        world
                            .resource_mut::<crate::ui::encyclopedia::places::SelectedPlace>()
                            .0 = Some(brackwater_id);
                        *world.resource_mut::<
                            crate::ui::encyclopedia::places::SelectedPlaceEntry,
                        >() = crate::ui::encyclopedia::places::SelectedPlaceEntry::Overview;
                    }
                }
                let property_mode = std::env::var("FISTFORCE_CAPTURE_PROPERTY").unwrap_or_default();
                if matches!(property_mode.as_str(), "1" | "permits" | "sale") {
                    if property_mode == "sale" {
                        world.insert_resource(
                            crate::ui::property_market::PropertyMarketTab::ForSale,
                        );
                    }
                    let hall = world
                        .query_filtered::<Entity, With<shared::components::Settlement>>()
                        .iter(world)
                        .find(|entity| {
                            world
                                .get::<shared::components::Settlement>(*entity)
                                .is_some_and(|settlement| settlement.name == "Brackwater")
                        });
                    if let Some(hall) = hall {
                        world.insert_resource(
                            crate::ui::property_market::PropertyMarketTarget(Some(hall)),
                        );
                        world.resource_mut::<crate::ui::player_permits::PendingPermitQuote>().0 =
                            Some(shared::protocol::HeroPermitQuote {
                                settlement: shared::components::SettlementId(1),
                                settlement_name: "Brackwater".into(),
                                kind: shared::components::SettlementBuildingKind::Farmstead,
                                fee: 165,
                                recommended_working_capital: 0,
                                wallet_balance: 1_000,
                                company: None,
                                company_cash: 0,
                            });
                    }
                }
                if let Ok(mode) = std::env::var("FISTFORCE_CAPTURE_HISTORY") {
                    let hall = world
                        .query_filtered::<Entity, With<shared::components::Settlement>>()
                        .iter(world)
                        .find(|entity| {
                            world
                                .get::<shared::components::Settlement>(*entity)
                                .is_some_and(|settlement| settlement.name == "Brackwater")
                        });
                    if let Some(hall) = hall {
                        if mode != "empty" {
                            let mut cache =
                                world.resource_mut::<crate::ui::history::SettlementHistoryCache>();
                            cache.archives.insert(
                                "Brackwater".to_string(),
                                synthetic_settlement_history("Brackwater"),
                            );
                            cache.world = Some(synthetic_world_history());
                        }
                        let _ = hall;
                    }
                }
            } else if mode == "coast" {
                use shared::components::SettlementBuildingKind as K;
                let rotation = 1.309_f32;
                let hut_ground = world
                    .get_resource::<shared::terrain::WorldTerrain>()
                    .map(|terrain| terrain.get_height(focus.x, focus.z))
                    .unwrap_or(focus.y);
                let hut = Vec3::new(focus.x, hut_ground, focus.z);
                let fisher = shared::names::person_name(8_001);
                let mut hut_store =
                    shared::economy::GoodsInventory::new(K::FishermansHut.storage_bulk_capacity());
                hut_store.add(shared::economy::Good::Food, 14);
                world.spawn((
                    shared::components::SettlementBuilding {
                        kind: K::FishermansHut,
                        settlement: "Brackwater".to_string(),
                        owner: Some(fisher.clone()),
                        quality: 0.43,
                        workers: vec![fisher],
                    },
                    hut_store,
                    shared::components::PlayerPosition(hut),
                    shared::components::PlayerRotation(rotation),
                ));

                let mut pier_at = K::FishermansHut
                    .pier_position(hut, rotation)
                    .expect("fisherman's hut has a pier anchor");
                pier_at.y = world
                    .get_resource::<shared::terrain::WorldTerrain>()
                    .and_then(|terrain| terrain.water_level())
                    .unwrap_or(0.0);
                world.spawn((
                    shared::components::FishingPier {
                        settlement: "Brackwater".to_string(),
                        fishermans_hut: hut,
                        quality: 0.43,
                    },
                    shared::components::PlayerPosition(pier_at),
                    shared::components::PlayerRotation(rotation),
                ));
            }
        });
    }

    // FISTFORCE_CAPTURE_HERO_CREATOR=1: open the character-creator modal so
    // captures can verify the live preview + selector UI without a server.
    if std::env::var("FISTFORCE_CAPTURE_HERO_CREATOR").is_ok_and(|v| v == "1") {
        commands.insert_resource(crate::ui::hero_creator::HeroCreatorOpen(true));
        if let Ok(preset) = std::env::var("FISTFORCE_CAPTURE_OUTFIT_PRESET") {
            commands.queue(move |world: &mut World| {
                let manifest = &world.resource::<crate::hero::HeroManifest>().0;
                let mut outfit = shared::components::HeroOutfit::from_manifest(manifest);
                manifest
                    .apply_outfit(&preset, &mut outfit)
                    .expect("capture outfit preset");
                world
                    .resource_mut::<crate::hero::control::SelectedOutfit>()
                    .0 = outfit;
            });
        }
    }

    // FISTFORCE_CAPTURE_WARBAR=1 stages three battalions with bearers and a
    // selection, so the battalion bar and standard flags can be photographed
    // without a server. Pair with FISTFORCE_COMBAT_MODE=1 and
    // FISTFORCE_CAPTURE_HUD=play so the war UI is armed and drawn.
    if std::env::var("FISTFORCE_CAPTURE_WARBAR").is_ok_and(|v| v == "1") {
        commands.insert_resource(crate::ui::name_entry::PlayerNameInput {
            name: "Wanderer".to_string(),
            submitted: true,
        });
        // Staged at the battle-field anchor; capture with --at -20,-40.
        let ground = |x: f32, z: f32| {
            terrain
                .as_deref()
                .map(|terrain| terrain.get_height(x, z))
                .unwrap_or(0.0)
        };
        let focus = Vec3::new(-20.0, ground(-20.0, -40.0), -40.0);
        let mut selected = Vec::new();
        for (ordinal, count, wounded) in [(1u64, 8usize, 0.0f32), (2, 6, 22.0), (3, 3, 55.0)] {
            let id = shared::components::BattalionId(ordinal);
            commands.spawn((
                shared::components::Battalion {
                    id,
                    name: format!("Battalion {ordinal}"),
                    ordinal,
                },
                shared::components::CommandedBy("wanderer".to_string()),
                shared::components::PlayerPosition(focus),
            ));
            for soldier in 0..count {
                let x = focus.x + (ordinal as f32 - 2.0) * 12.0 + (soldier % 4) as f32 * 1.5;
                let z = focus.z + (soldier / 4) as f32 * 1.7 - 6.0;
                let position = Vec3::new(x, ground(x, z), z);
                let mut body = commands.spawn((
                    shared::components::CharacterName(format!("Soldier {ordinal}-{soldier}")),
                    shared::components::CharacterKind::Villager,
                    shared::components::PlayerPosition(position),
                    shared::components::CharacterAttributes::from_seed(
                        ordinal * 31 + soldier as u64,
                    ),
                    {
                        // Wounded, not weaker: full max, reduced current.
                        let mut health = shared::components::Health::new(
                            shared::components::CHARACTER_MAX_HEALTH,
                        );
                        health.current -= wounded;
                        health
                    },
                    shared::components::CommandedBy("wanderer".to_string()),
                    shared::components::MemberOfBattalion(id),
                ));
                if soldier == 0 {
                    body.insert(shared::components::StandardBearer);
                }
                if ordinal == 2 {
                    selected.push(body.id());
                }
            }
        }
        commands.insert_resource(crate::selection::Selection::from_entities(selected));
    }

    // FISTFORCE_CAPTURE_ENCYCLOPEDIA=1 opens the encyclopedia and seeds a
    // sample cast, so the window can be verified without a server (there is no
    // roster offline, and an empty list photographs nothing).
    if let Ok(mode) = std::env::var("FISTFORCE_CAPTURE_ENCYCLOPEDIA") {
        use crate::ui::encyclopedia::{
            Affiliation, EncyclopediaOpen, KnownPeople, PersonKind, PersonRecord, SelectedPerson,
        };
        // "live" opens the window with NO sample cast, so it fills from the
        // characters actually spawned in the world. That is the only way to
        // verify the real path -- a seeded list proves the layout renders and
        // nothing about whether characters reach it.
        if mode == "live" || mode == "places" {
            commands.insert_resource(EncyclopediaOpen(true));
            commands.insert_resource(crate::ui::hud::GodCapability(true));
            if mode == "places" {
                commands.insert_resource(crate::ui::encyclopedia::EncyclopediaTab::Places);
            }
            return;
        }
        let sample =
            |name: &str, level, prestige, online, known, is_self, affiliation| PersonRecord {
                id: shared::components::PersonId::UNASSIGNED,
                name: name.to_string(),
                kind: PersonKind::Hero,
                affiliation,
                level,
                prestige,
                online,
                alive: true,
                health: Some(shared::components::Health::default()),
                death_day: None,
                death_cause: None,
                known,
                is_self,
                commanded_by: None,
                residence: None,
                home: None,
                occupation: None,
                workplace: None,
                wallet: None,
                nutrition: None,
                activity: None,
                objective: None,
                day_plan: None,
                navigation: None,
                attributes: Some(shared::components::CharacterAttributes::default()),
                work_status: Some(shared::components::WorkStatus::LookingForWork),
                daily_wage: None,
                workforce_requirements: None,
                inventory: Some(shared::economy::GoodsInventory::new(
                    shared::economy::capacity::VILLAGER,
                )),
                carried: Some(shared::economy::CarriedLoad::default()),
            };
        let mut records = vec![
            sample("Aldric", 7, 2, true, true, true, Affiliation::default()),
            sample("Bryn", 4, 0, true, true, false, Affiliation::default()),
            sample(
                "Cassia",
                11,
                5,
                false,
                true,
                false,
                shared::components::CharacterAffiliation(Some(0)),
            ),
            sample("Dunstan", 2, 0, false, true, false, Affiliation::default()),
            sample(
                "Eirwen",
                9,
                3,
                true,
                true,
                false,
                shared::components::CharacterAffiliation(Some(0)),
            ),
            sample("Faelan", 1, 0, false, false, false, Affiliation::default()),
            sample(
                "Gwyneth",
                14,
                8,
                false,
                false,
                false,
                shared::components::CharacterAffiliation(Some(0)),
            ),
            sample("Hollis", 5, 1, false, true, false, Affiliation::default()),
            sample("Ivo", 3, 0, false, false, false, Affiliation::default()),
            sample(
                "Jorunn",
                8,
                4,
                true,
                true,
                false,
                shared::components::CharacterAffiliation(Some(0)),
            ),
            sample("Kelda", 6, 2, false, true, false, Affiliation::default()),
            sample("Lorcan", 12, 6, false, false, false, Affiliation::default()),
        ];
        for (index, record) in records.iter_mut().enumerate() {
            record.id = shared::components::PersonId(index as u64 + 1);
            if record.is_self {
                record.wallet = Some(2_750);
            }
        }
        let cassia = records
            .iter()
            .find(|record| record.name == "Cassia")
            .map(|record| record.id);
        commands.insert_resource(KnownPeople {
            records,
            requested: true,
        });
        commands.insert_resource(SelectedPerson(cassia));
        commands.insert_resource(EncyclopediaOpen(true));
        // Mode picks which surface to photograph: a tab name, or "god" to
        // grant capability so the unknown-people view can be verified.
        commands.insert_resource(match mode.as_str() {
            "retinue" => crate::ui::encyclopedia::EncyclopediaTab::Retinue,
            "army" => crate::ui::encyclopedia::EncyclopediaTab::Army,
            "ledger" | "companies" | "company-stock" | "business" | "founding" => {
                crate::ui::encyclopedia::EncyclopediaTab::Companies
            }
            _ => crate::ui::encyclopedia::EncyclopediaTab::People,
        });
        if mode == "retinue" {
            // A photographable clan: claim the account name the roster rows
            // compare against, then swear three staged villagers to it. Pair
            // with FISTFORCE_CAPTURE_HERO=default FISTFORCE_CAPTURE_SELECT=1
            // for the hero row itself.
            commands.insert_resource(crate::ui::name_entry::PlayerNameInput {
                name: "Wanderer".to_string(),
                submitted: true,
            });
            for (index, (name, occupation, offset)) in [
                ("Odo Sverreson", "Fisher", Vec3::new(6.0, 0.0, 4.0)),
                ("Brenna the Mason", "Mason", Vec3::new(-5.0, 0.0, 7.0)),
                ("Wystan the Elder", "Farmer", Vec3::new(2.0, 0.0, -6.0)),
            ]
            .into_iter()
            .enumerate()
            {
                commands.spawn((
                    shared::components::CharacterName(name.to_string()),
                    shared::components::PersonId(9_100 + index as u64),
                    shared::components::CharacterKind::Villager,
                    shared::components::PlayerPosition(offset),
                    shared::components::CharacterActivity::Idle,
                    shared::components::CharacterObjective::WalkingAroundTown,
                    shared::components::Occupation(Some(occupation.to_string())),
                    shared::components::CommandedBy("wanderer".to_string()),
                ));
            }
        }
        if mode == "army" {
            // A photographable army: the tab reads live replicated components,
            // so stage real entities - two battalions plus a mixed roster with
            // varied physique, and a couple of unassigned conscripts.
            commands.insert_resource(crate::ui::name_entry::PlayerNameInput {
                name: "Wanderer".to_string(),
                submitted: true,
            });
            // UI-only fixture: roles exercise roster/transfer controls. Mounted
            // movement and horse pairing require the separate connected lab.
            let cavalry =
                std::env::var("FISTFORCE_CAPTURE_ARMY_CAVALRY").is_ok_and(|value| value == "1");
            for (id, name) in [(1u64, "1st Battalion"), (2u64, "2nd Battalion")] {
                commands.spawn((
                    shared::components::Battalion {
                        id: shared::components::BattalionId(id),
                        name: name.to_string(),
                        ordinal: id,
                    },
                    shared::components::CommandedBy("wanderer".to_string()),
                    if cavalry && id == 1 {
                        shared::components::SoldierRole::Cavalry
                    } else {
                        shared::components::SoldierRole::Infantry
                    },
                    shared::components::PlayerPosition(Vec3::new(id as f32 * 30.0, 0.0, 0.0)),
                ));
            }
            for (index, (name, physique, battalion)) in [
                ("Odo Sverreson", 19u8, Some(1u64)),
                ("Brenna the Mason", 16, Some(1)),
                ("Wystan the Elder", 11, Some(1)),
                ("Halvar Ironhand", 20, Some(2)),
                ("Kelda of the Ford", 13, Some(2)),
                ("Ivo the Younger", 15, None),
                ("Sigrun Half-Song", 9, None),
            ]
            .into_iter()
            .enumerate()
            {
                let mut soldier = commands.spawn((
                    shared::components::CharacterName(name.to_string()),
                    shared::components::PersonId(9_200 + index as u64),
                    shared::components::CharacterKind::Villager,
                    shared::components::PlayerPosition(Vec3::new(index as f32 * 2.0, 0.0, 4.0)),
                    shared::components::CharacterAttributes::from_seed(physique as u64 * 37),
                    shared::components::Health::new(
                        shared::components::CHARACTER_MAX_HEALTH - index as f32 * 9.0,
                    ),
                    shared::components::CommandedBy("wanderer".to_string()),
                    if cavalry && (battalion == Some(1) || index == 5) {
                        shared::components::SoldierRole::Cavalry
                    } else {
                        shared::components::SoldierRole::Infantry
                    },
                ));
                if let Some(battalion) = battalion {
                    soldier.insert(shared::components::MemberOfBattalion(
                        shared::components::BattalionId(battalion),
                    ));
                }
            }
        }
        if mode == "founding" {
            commands.insert_resource(crate::ui::company_founding::FoundingPageOpen(true));
            // FISTFORCE_CAPTURE_FOUNDING_NAME=<text> photographs the field mid-edit
            // with that text (empty = just the caret). The stand-in hero is
            // PersonId 10_000, so the page does not re-seed the draft.
            if let Ok(name) = std::env::var("FISTFORCE_CAPTURE_FOUNDING_NAME") {
                commands.insert_resource(crate::ui::company_founding::CompanyFoundingDraft {
                    founder: Some(shared::components::PersonId(10_000)),
                    name,
                    editing_name: true,
                    ..default()
                });
            }
        }
        if matches!(
            mode.as_str(),
            "ledger" | "companies" | "company-stock" | "business"
        ) {
            stage_capture_companies(&mut commands);
            commands.insert_resource(crate::ui::encyclopedia::companies::SelectedCompany(Some(
                shared::components::CompanyId(501),
            )));
        }
        if mode == "god" {
            commands.insert_resource(crate::ui::hud::GodCapability(true));
        }
    }

    info!("capture: entering world offline (no server)");
}
