//! Offscreen visual capture — screenshots of the real renderer, without a server.
//!
//! Compiling is not proof. Terrain winding, shader artifacts, foliage orientation,
//! lighting and material bugs all build clean and only show up on screen, so this binary
//! exists to make "does it actually look right?" answerable without a human watching.
//!
//! It boots the client's real rendering stack (terrain, water, props, sky, atmosphere)
//! but skips networking entirely: `WorldTerrain` loads the map from disk, and the
//! commander camera is the streaming anchor, so no server is needed. A local `WorldTime`
//! entity stands in for the replicated one so the time of day is controllable.
//!
//! Run it via `cargo run -p client --bin capture -- --help`.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use shared::components::WorldTime;

use crate::camera_rts::CommanderCamera;
use crate::states::GameState;
use crate::terrain::LoadedChunks;

/// One camera placement to photograph.
#[derive(Debug, Clone)]
pub struct Shot {
    /// Used as the output filename stem.
    pub name: String,
    /// World-space point the camera centers on.
    pub focus: Vec3,
    /// Camera yaw in radians.
    pub yaw: f32,
    /// Distance from the focus point.
    pub zoom: f32,
    /// Downward tilt in radians. Lower = more horizon, higher = more top-down.
    pub tilt: f32,
    /// Time of day in `0.0..=1.0` (0.5 = noon).
    pub time_of_day: f32,
}

impl Default for Shot {
    fn default() -> Self {
        Self {
            name: "shot".to_string(),
            focus: Vec3::ZERO,
            yaw: -0.45,
            zoom: 220.0,
            tilt: 0.75,
            time_of_day: 0.5,
        }
    }
}

#[derive(Resource, Debug, Clone)]
pub struct CaptureConfig {
    pub out_dir: PathBuf,
    pub shots: Vec<Shot>,
    /// Frames to render before the first shot, so terrain chunks, props and textures
    /// have time to stream in. Streaming is async — too few frames photographs a
    /// half-loaded world, which looks like a rendering bug but is not one.
    pub warmup_frames: u32,
    /// Frames between moving the camera and taking the shot.
    pub settle_frames: u32,
}

/// Where we are in the capture sequence.
#[derive(Resource, Debug)]
enum CaptureState {
    /// Letting the world stream in before the first shot.
    Warmup {
        frames_left: u32,
    },
    /// Camera moved, waiting for the world to settle at the new location.
    Settling {
        shot: usize,
        frames_left: u32,
    },
    /// Screenshot requested; waiting for the file to actually exist on disk.
    ///
    /// This is the latch that matters: the render-to-disk round trip is async, so
    /// exiting on a frame count instead races the writer and truncates the last image.
    AwaitingFile {
        shot: usize,
        path: PathBuf,
        frames_waited: u32,
    },
    Done,
}

pub fn run(config: CaptureConfig) {
    // Captures must not inherit the user's saved settings file.
    std::env::set_var("FISTFORCE_NO_SETTINGS_FILE", "1");
    if let Err(e) = std::fs::create_dir_all(&config.out_dir) {
        eprintln!(
            "capture: cannot create output dir {}: {e}",
            config.out_dir.display()
        );
        std::process::exit(1);
    }

    let asset_path = crate::get_asset_path();
    let mut app = App::new();
    crate::app_wiring::setup_plugins(&mut app, asset_path);
    crate::app_wiring::setup_resources(&mut app);
    crate::app_wiring::setup_systems(&mut app);

    app.insert_resource(CaptureState::Warmup {
        frames_left: config.warmup_frames,
    });
    app.insert_resource(config);

    app.add_systems(Startup, enter_world_offline);
    app.add_systems(
        Update,
        (
            spawn_capture_heroes,
            exercise_capture_door,
            select_capture_person,
            select_capture_place,
            open_capture_history,
            force_capture_drag_box,
            drive_capture,
        ),
    );

    app.run();
}

/// `FISTFORCE_CAPTURE_DOOR=open` holds the offline settlement's town-hall
/// door open through the same stable building-side state used in a live game.
/// This is intentionally independent of villager AI and networking: a capture
/// made with it is a smoke test for scene instantiation, graph wiring, the
/// replicated demand consumer, and the authored glTF clip in one repeatable run.
fn exercise_capture_door(
    mut commands: Commands,
    settlements: Query<Entity, With<shared::components::Settlement>>,
    mut applied: Local<bool>,
) {
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
fn enter_world_offline(mut commands: Commands, mut next_state: ResMut<NextState<GameState>>) {
    // No connection, no name entry — the map comes off disk.
    next_state.set(GameState::Playing);

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
    if let Ok(mode) = std::env::var("FISTFORCE_CAPTURE_HUD") {
        if !mode.is_empty() {
            if mode == "god" {
                commands.insert_resource(crate::ui::hud::GodCapability(true));
                commands.insert_resource(crate::ui::hud::HudMode::God);
            }
        }
    }

    // FISTFORCE_CAPTURE_SETTLEMENT=1 founds a settlement at the shot's focus so
    // the moot hall can be photographed without a server.
    // "village" additionally populates the first one; "coast" stages the
    // deterministic Village Lab hut/pier pair for shoreline inspection.
    if std::env::var("FISTFORCE_CAPTURE_SETTLEMENT")
        .is_ok_and(|v| v == "1" || v == "village" || v == "coast")
    {
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
            let settlement_focus = if mode == "coast" {
                // The requested focus is the hut. This is its deterministic
                // offset from the lab hall selected on village_lab seed 3.
                focus - Vec3::new(53.94803, 0.0, -41.39578)
            } else {
                focus
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
                            residents: (i as u32) * 3,
                            treasury: 0,
                        },
                        shared::components::PlayerPosition(Vec3::new(at.x, y, at.z)),
                    ))
                    .id();
                if i == 0 && mode == "village" {
                    let mut store =
                        shared::economy::GoodsInventory::new(shared::economy::capacity::HALL);
                    store.add(shared::economy::Good::Food, 9);
                    store.add(shared::economy::Good::Wheat, 18);
                    store.add(shared::economy::Good::Wood, 7);
                    store.add(shared::economy::Good::Stone, 2);
                    let mut market = shared::economy::MootMarket::founding();
                    market.refresh_all(&store);
                    world.entity_mut(hall).insert((
                        store,
                        market,
                        shared::components::MootAdministration {
                            road_steward: Some(shared::names::person_name(7_002)),
                            roadless_buildings: 1,
                            disconnected_buildings: 0,
                            last_road_audit_day: 12,
                            ..default()
                        },
                        shared::components::SettlementPolicies::poor_relief(),
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
                        },
                    ));
                }
            }

            // FISTFORCE_CAPTURE_SETTLEMENT=village also populates the FIRST
            // settlement: residents on record, buildings standing, one going
            // up. These are offline stand-ins for what the server's autonomy
            // produces, so the settlement panel can be photographed without
            // waiting out a live village.
            if std::env::var("FISTFORCE_CAPTURE_SETTLEMENT").is_ok_and(|v| v == "village") {
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
                        },
                        shared::components::CharacterActivity::Indoors,
                    ));
                }
                for (index, kind) in [K::Farmstead, K::LumberjackHut].into_iter().enumerate() {
                    let at = focus + Vec3::new(30.0 + index as f32 * 14.0, 0.0, 18.0);
                    let ground = world
                        .get_resource::<shared::terrain::WorldTerrain>()
                        .map(|t| t.get_height(at.x, at.z))
                        .unwrap_or(at.y);
                    let mut store =
                        shared::economy::GoodsInventory::new(kind.storage_bulk_capacity());
                    if kind == K::Farmstead {
                        store.add(shared::economy::Good::Wheat, 11);
                    } else {
                        store.add(shared::economy::Good::Wood, 6);
                    }
                    world.spawn((
                        shared::components::SettlementBuilding {
                            kind,
                            settlement: "Brackwater".to_string(),
                            owner: Some(people[index].clone()),
                            // Stand-ins, like the resident names above. The
                            // client has no BiomeField truth to sample and must
                            // not invent one -- these numbers exist so the panel
                            // has something to lay out, nothing more.
                            quality: if index == 0 { 0.82 } else { 0.41 },
                            workers: vec![people[index].clone()],
                        },
                        shared::components::PlayerPosition(Vec3::new(at.x, ground, at.z)),
                        shared::components::PlayerRotation(0.0),
                        store,
                    ));
                }
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
                if let Some(mut settlement) = world
                    .query::<&mut shared::components::Settlement>()
                    .iter_mut(world)
                    .find(|s| s.name == "Brackwater")
                {
                    settlement.residents = people.len() as u32;
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
                        world.insert_resource(crate::ui::settlement_panel::TradePanelTarget(Some(
                            hall,
                        )));
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
                attributes: Some(shared::components::CharacterAttributes::default()),
                work_status: Some(shared::components::WorkStatus::LookingForWork),
                daily_wage: None,
                workforce_requirements: None,
                inventory: Some(shared::economy::GoodsInventory::new(
                    shared::economy::capacity::VILLAGER,
                )),
                carried: Some(shared::economy::CarriedLoad::default()),
            };
        commands.insert_resource(KnownPeople {
            records: vec![
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
            ],
            requested: true,
        });
        commands.insert_resource(SelectedPerson(Some("Cassia".to_string())));
        commands.insert_resource(EncyclopediaOpen(true));
        // Mode picks which surface to photograph: a tab name, or "god" to
        // grant capability so the unknown-people view can be verified.
        commands.insert_resource(match mode.as_str() {
            "retinue" => crate::ui::encyclopedia::EncyclopediaTab::Retinue,
            "ledger" => crate::ui::encyclopedia::EncyclopediaTab::Ledger,
            _ => crate::ui::encyclopedia::EncyclopediaTab::People,
        });
        if mode == "god" {
            commands.insert_resource(crate::ui::hud::GodCapability(true));
        }
    }

    info!("capture: entering world offline (no server)");
}

/// Open history after the commander camera has rendered ordinary world frames.
/// A modal present on the very first offline capture frame prevents the capture
/// harness's camera from completing its initial convergence; real players can
/// only open this after entering the world, so the delay mirrors actual use.
fn open_capture_history(
    settlements: Query<(Entity, &shared::components::Settlement)>,
    mut target: ResMut<crate::ui::history::HistoryPanelTarget>,
    mut frames: Local<u8>,
) {
    let Ok(mode) = std::env::var("FISTFORCE_CAPTURE_HISTORY") else {
        return;
    };
    if target.0.is_some() {
        return;
    }
    *frames = frames.saturating_add(1);
    if *frames < 30 {
        return;
    }
    let hall = settlements
        .iter()
        .find(|(_, settlement)| settlement.name == "Brackwater")
        .map(|(entity, _)| entity);
    let view = match mode.as_str() {
        "market" => crate::ui::history::HistoryView::Market(shared::economy::Good::Wood),
        "world" => crate::ui::history::HistoryView::World,
        _ => crate::ui::history::HistoryView::Village,
    };
    let settlement = if view == crate::ui::history::HistoryView::World {
        None
    } else {
        hall
    };
    if view != crate::ui::history::HistoryView::World && settlement.is_none() {
        return;
    }
    target.0 = Some(crate::ui::history::HistoryTarget {
        settlement,
        place: if view == crate::ui::history::HistoryView::World {
            "World".to_string()
        } else {
            "Brackwater".to_string()
        },
        view,
        return_to_trade: false,
    });
}

/// FISTFORCE_CAPTURE_HERO spawns stand-in heroes (offline fakes of the
/// replicated entity) in a line at the first shot's focus, terrain-snapped, so
/// captures can verify the character model, wardrobe and pose without a
/// server.
///
/// Spec: `slot0,slot1,...,skin` per hero, semicolon-separated, all indices
/// into the manifest's slot items / skin tones (order as in Humanoid.ron:
/// bottom, top, hair). Missing or unparsable fields use the manifest default.
/// `FISTFORCE_CAPTURE_HERO=default` spawns one hero in the declared default.
fn spawn_capture_heroes(
    mut commands: Commands,
    config: Res<CaptureConfig>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
    mut spawned: Local<bool>,
) {
    if *spawned {
        return;
    }
    let mut hero_spec = std::env::var("FISTFORCE_CAPTURE_HERO").ok();
    let villager_count = std::env::var("FISTFORCE_CAPTURE_VILLAGERS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok());
    // Comma-separated authored bundle appearances. Supplying this alone
    // creates one default hero, making the first-integration WoodBundle shot a
    // one-flag exercise rather than a bespoke capture path.
    let carried = std::env::var("FISTFORCE_CAPTURE_CARRIED")
        .ok()
        .map(|spec| {
            spec.split(',')
                .filter_map(|raw| match raw.trim().to_ascii_lowercase().as_str() {
                    "wood" => Some((
                        shared::economy::Good::Wood,
                        shared::economy::CarriedAppearance::WoodBundle,
                    )),
                    "wheat" => Some((
                        shared::economy::Good::Wheat,
                        shared::economy::CarriedAppearance::WheatSheaf,
                    )),
                    "fish" | "food" => Some((
                        shared::economy::Good::Food,
                        shared::economy::CarriedAppearance::FishBasket,
                    )),
                    "stone" => Some((
                        shared::economy::Good::Stone,
                        shared::economy::CarriedAppearance::StoneBundle,
                    )),
                    "iron" => Some((
                        shared::economy::Good::Iron,
                        shared::economy::CarriedAppearance::IronBundle,
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    // Comma-separated visible work states. This drives the same replicated
    // activity component as a live village, so authored clips and hand tools
    // can be reviewed offline without waiting for a worker cycle.
    let activities = std::env::var("FISTFORCE_CAPTURE_ACTIVITY")
        .ok()
        .map(|spec| {
            spec.split(',')
                .filter_map(|raw| match raw.trim().to_ascii_lowercase().as_str() {
                    "build" | "building" => Some(shared::components::CharacterActivity::Building),
                    "chop" | "chopping" => Some(shared::components::CharacterActivity::Chopping),
                    "farm" | "farming" | "harvest" => {
                        Some(shared::components::CharacterActivity::Farming)
                    }
                    "fish" | "fishing" => Some(shared::components::CharacterActivity::Fishing),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if hero_spec.is_none()
        && villager_count.is_none()
        && carried.is_empty()
        && activities.is_empty()
    {
        *spawned = true;
        return;
    }
    if hero_spec.is_none() && (!carried.is_empty() || !activities.is_empty()) {
        let fixture_count = carried.len().max(activities.len()).max(1);
        hero_spec = Some(vec!["default"; fixture_count].join(";"));
    }
    let Some(terrain) = terrain else {
        return;
    };
    let base = config.shots.first().map(|s| s.focus).unwrap_or(Vec3::ZERO);
    let spec = hero_spec.unwrap_or_default();
    // Indices are validated against the same manifest the renderer uses.
    let manifest = match shared::character::CharacterManifest::load() {
        Ok(manifest) => manifest,
        Err(e) => {
            error!("capture: character manifest unavailable: {e}");
            *spawned = true;
            return;
        }
    };
    let default_outfit = shared::components::HeroOutfit::from_manifest(&manifest);
    for (i, outfit_spec) in spec.split(';').filter(|s| !s.is_empty()).enumerate() {
        // Positional parse: a bad token falls back to the default for THAT
        // field instead of shifting later fields left.
        let parts: Vec<Option<u8>> = outfit_spec
            .split(',')
            .map(|p| p.trim().parse().ok())
            .collect();
        let mut outfit = default_outfit;
        for (slot_index, _) in manifest.slots.iter().enumerate() {
            if let Some(Some(value)) = parts.get(slot_index) {
                outfit.slots[slot_index] = *value;
            }
        }
        if let Some(Some(skin)) = parts.get(manifest.slots.len()) {
            outfit.skin = *skin;
        }
        let x = base.x + i as f32 * 1.4;
        let z = base.z;
        let pos = Vec3::new(x, terrain.get_height(x, z), z);
        let entity = commands
            .spawn((
                shared::components::Hero {
                    owner: lightyear::prelude::PeerId::Netcode(1000 + i as u64),
                },
                shared::components::CharacterName(format!("Capture Hero {}", i + 1)),
                shared::components::CharacterKind::Hero,
                shared::components::CharacterAffiliation::default(),
                outfit,
                shared::components::PlayerPosition(pos),
                shared::components::PlayerRotation(std::f32::consts::PI),
            ))
            .id();
        if let Some((good, appearance)) = carried.get(i % carried.len().max(1)).copied() {
            commands
                .entity(entity)
                .insert(shared::economy::CarriedLoad {
                    good: Some(good),
                    amount: 1,
                    appearance: Some(appearance),
                });
        }
        if let Some(activity) = activities.get(i % activities.len().max(1)).copied() {
            commands.entity(entity).insert(activity);
        }
    }
    // FISTFORCE_CAPTURE_VILLAGERS=<n> drops n named villagers in a row behind the
    // heroes, so the encyclopedia and the character visuals can be verified
    // without a server. Names come from the same generator the server uses.
    if let Some(count) = villager_count {
        for i in 0..count {
            let x = base.x + i as f32 * 1.5 - (count as f32 * 0.75);
            let z = base.z + 3.0;
            let pos = Vec3::new(x, terrain.get_height(x, z), z);
            let seed = 1_000 + i as u64;
            let entity = commands
                .spawn((
                    shared::components::CharacterName(shared::names::person_name(seed)),
                    shared::components::CharacterKind::Villager,
                    shared::components::CharacterAffiliation::default(),
                    shared::components::HeroOutfit::varied(seed),
                    shared::components::PlayerPosition(pos),
                    shared::components::PlayerRotation(std::f32::consts::PI),
                ))
                .id();
            if let Some((good, appearance)) = carried.get(i % carried.len().max(1)).copied() {
                commands
                    .entity(entity)
                    .insert(shared::economy::CarriedLoad {
                        good: Some(good),
                        amount: 1,
                        appearance: Some(appearance),
                    });
            }
            if let Some(activity) = activities.get(i % activities.len().max(1)).copied() {
                commands.entity(entity).insert(activity);
            }
        }
    }

    // FISTFORCE_CAPTURE_SELECT=1 selects the FIRST fake hero, so the ground ring
    // and the selected-unit plate can be verified without a server. Deferred by
    // a command so it runs after the spawns above are applied.
    // FISTFORCE_CAPTURE_SELECT=all force-selects EVERY character, including ones
    // you do not own. This is a stress harness for the ring pool and the group
    // HUD, NOT a picture of what a box-drag produces: a real drag filters to
    // your own units (see selection::pick). Do not read it as the game's rule.
    if std::env::var("FISTFORCE_CAPTURE_SELECT").is_ok_and(|v| v == "all") {
        commands.queue(|world: &mut World| {
            let mut all = world.query_filtered::<Entity, With<shared::components::CharacterName>>();
            let entities: Vec<Entity> = all.iter(world).collect();
            if let Some((first, hero)) = world
                .query::<(Entity, &shared::components::Hero)>()
                .iter(world)
                .next()
            {
                let owner = shared::player::peer_id_to_u64(hero.owner);
                let _ = first;
                world.insert_resource(crate::camera_rts::LocalPeerId(owner));
            }
            world.resource_mut::<crate::selection::Selection>().entities = entities;
        });
    } else if std::env::var("FISTFORCE_CAPTURE_SELECT").is_ok_and(|v| v == "market") {
        // Prefer the hall carrying the actual market components. Useful when a
        // staged capture also contains several distant list-only settlements.
        commands.queue(|world: &mut World| {
            let entity = world
                .query_filtered::<Entity, (
                    With<shared::components::Settlement>,
                    With<shared::economy::MootMarket>,
                )>()
                .iter(world)
                .next();
            if let Some(entity) = entity {
                world.resource_mut::<crate::selection::Selection>().entities = vec![entity];
            }
        });
    } else if std::env::var("FISTFORCE_CAPTURE_SELECT").is_ok_and(|v| v == "hall") {
        // Selects the first SETTLEMENT rather than a person, so the settlement
        // panel can be photographed. A place is never commandable, so this is
        // always a single selection.
        commands.queue(|world: &mut World| {
            let entity = world
                .query_filtered::<Entity, With<shared::components::Settlement>>()
                .iter(world)
                .next();
            if let Some(entity) = entity {
                world.resource_mut::<crate::selection::Selection>().entities = vec![entity];
            }
        });
    } else if std::env::var("FISTFORCE_CAPTURE_SELECT").is_ok_and(|v| v == "1") {
        commands.queue(|world: &mut World| {
            let mut heroes = world.query_filtered::<(Entity, &shared::components::Hero), ()>();
            if let Some((first, hero)) = heroes.iter(world).next() {
                let owner = shared::player::peer_id_to_u64(hero.owner);
                world.resource_mut::<crate::selection::Selection>().entities = vec![first];
                let _ = &owner;
                // Claim ownership of it too, so the shot shows the state a real
                // player sees (ember mark, own name) rather than "NOT YOURS".
                world.insert_resource(crate::camera_rts::LocalPeerId(owner));
            }
        });
    }
    *spawned = true;
}

/// FISTFORCE_CAPTURE_DRAG_BOX="x0,y0,x1,y1[,ui_scale]" pins the drag-select
/// marquee to a known rectangle in WINDOW pixels, so where it actually lands on
/// screen can be measured instead of eyeballed.
///
/// The optional ui_scale reproduces the macOS setup, where the window takes a
/// scale-factor override of 1.0 and the Retina factor lives in `UiScale` -- the
/// exact condition under which cursor pixels and UI pixels stop being the same
/// unit, which is what put the marquee in the wrong place.
fn force_capture_drag_box(
    mut drag: ResMut<crate::selection::DragBox>,
    mut ui_scale: ResMut<bevy::ui::UiScale>,
) {
    let Ok(spec) = std::env::var("FISTFORCE_CAPTURE_DRAG_BOX") else {
        return;
    };
    let parts: Vec<f32> = spec
        .split(',')
        .filter_map(|p| p.trim().parse().ok())
        .collect();
    if parts.len() < 4 {
        return;
    }
    if let Some(scale) = parts.get(4) {
        if ui_scale.0 != *scale {
            ui_scale.0 = *scale;
        }
    }
    drag.start = Some(Vec2::new(parts[0], parts[1]));
    drag.current = Vec2::new(parts[2], parts[3]);
    drag.active = true;
}

/// FISTFORCE_CAPTURE_SELECT_PERSON=<name> selects that person in the
/// encyclopedia once they actually exist.
///
/// Retried rather than set once: `rebuild_people_list` drops a selection that is
/// not in the visible list, and at startup the list is empty, so a one-shot set
/// is cleared before the characters have even been learned.
/// FISTFORCE_CAPTURE_SELECT_PLACE=<name>, retried for the same reason as the
/// person selector: the list is empty at startup and drops a selection it does
/// not contain.
fn select_capture_place(
    places: Res<crate::ui::encyclopedia::places::KnownPlaces>,
    mut selected: ResMut<crate::ui::encyclopedia::places::SelectedPlace>,
    mut entry: ResMut<crate::ui::encyclopedia::places::SelectedPlaceEntry>,
) {
    let Ok(wanted) = std::env::var("FISTFORCE_CAPTURE_SELECT_PLACE") else {
        return;
    };
    let Some(place) = places.find(&wanted) else {
        return;
    };
    if selected.0.as_deref() != Some(wanted.as_str()) {
        selected.0 = Some(wanted);
        *entry = crate::ui::encyclopedia::places::SelectedPlaceEntry::Overview;
    }
    let Ok(building) = std::env::var("FISTFORCE_CAPTURE_SELECT_BUILDING") else {
        return;
    };
    if building.eq_ignore_ascii_case("overview") {
        *entry = crate::ui::encyclopedia::places::SelectedPlaceEntry::Overview;
    } else if building.eq_ignore_ascii_case("hall")
        || building.eq_ignore_ascii_case(shared::components::SettlementBuildingKind::Hall.label())
    {
        *entry = crate::ui::encyclopedia::places::SelectedPlaceEntry::Hall;
    } else if let Some((index, _)) = place
        .buildings
        .iter()
        .enumerate()
        .find(|(_, record)| record.kind.label().eq_ignore_ascii_case(&building))
    {
        *entry = crate::ui::encyclopedia::places::SelectedPlaceEntry::Building(index);
    }
}

fn select_capture_person(
    people: Res<crate::ui::encyclopedia::KnownPeople>,
    mut selected: ResMut<crate::ui::encyclopedia::SelectedPerson>,
) {
    if selected.0.is_some() {
        return;
    }
    let Ok(wanted) = std::env::var("FISTFORCE_CAPTURE_SELECT_PERSON") else {
        return;
    };
    if wanted.eq_ignore_ascii_case("first") {
        if let Some(record) = people.records.iter().find(|record| record.known) {
            selected.0 = Some(record.name.clone());
        }
        return;
    }
    if wanted.eq_ignore_ascii_case("hungry") {
        if let Some(record) = people.records.iter().find(|record| {
            record
                .nutrition
                .is_some_and(|nutrition| nutrition.is_hungry())
        }) {
            selected.0 = Some(record.name.clone());
        }
        return;
    }
    if people.find(&wanted).is_some() {
        selected.0 = Some(wanted);
    }
}

#[allow(clippy::too_many_arguments)]
fn drive_capture(
    mut commands: Commands,
    config: Res<CaptureConfig>,
    mut state: ResMut<CaptureState>,
    mut cameras: Query<&mut CommanderCamera>,
    mut world_time: Query<&mut WorldTime>,
    loaded_chunks: Option<Res<LoadedChunks>>,
    mut app_exit: MessageWriter<AppExit>,
) {
    match &mut *state {
        CaptureState::Warmup { frames_left } => {
            // Park the camera on the first shot during warmup so streaming loads the
            // right chunks rather than whatever is around the origin.
            if let Some(shot) = config.shots.first() {
                apply_shot(shot, &mut cameras, &mut world_time);
            }

            if *frames_left > 0 {
                *frames_left -= 1;
                return;
            }
            info!(
                "capture: warmup complete, {} shot(s) queued",
                config.shots.len()
            );
            *state = CaptureState::Settling {
                shot: 0,
                frames_left: config.settle_frames,
            };
        }

        CaptureState::Settling { shot, frames_left } => {
            let index = *shot;
            let Some(current) = config.shots.get(index) else {
                *state = CaptureState::Done;
                return;
            };
            apply_shot(current, &mut cameras, &mut world_time);

            if *frames_left > 0 {
                *frames_left -= 1;
                return;
            }

            info!(
                "capture: '{}' focus={:?} zoom={} tilt={} time={} | {} terrain chunks loaded",
                current.name,
                current.focus,
                current.zoom,
                current.tilt,
                current.time_of_day,
                loaded_chunks.map(|c| c.chunks.len()).unwrap_or(0),
            );
            let path = config.out_dir.join(format!("{}.png", current.name));
            let _ = std::fs::remove_file(&path);
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path.clone()));
            info!("capture: shooting '{}' -> {}", current.name, path.display());
            *state = CaptureState::AwaitingFile {
                shot: index,
                path,
                frames_waited: 0,
            };
        }

        CaptureState::AwaitingFile {
            shot,
            path,
            frames_waited,
        } => {
            // Latch on the file existing rather than a frame count, so a slow write
            // cannot leave us with a truncated PNG.
            let written = std::fs::metadata(&*path)
                .map(|m| m.len() > 0)
                .unwrap_or(false);
            *frames_waited += 1;

            if !written {
                if *frames_waited > 600 {
                    error!("capture: '{}' never hit disk, giving up", path.display());
                } else {
                    return;
                }
            } else {
                info!("capture: wrote {}", path.display());
            }

            let next = *shot + 1;
            if next >= config.shots.len() {
                info!("capture: all {} shot(s) complete", config.shots.len());
                *state = CaptureState::Done;
            } else {
                *state = CaptureState::Settling {
                    shot: next,
                    frames_left: config.settle_frames,
                };
            }
        }

        CaptureState::Done => {
            app_exit.write(AppExit::Success);
        }
    }
}

fn apply_shot(
    shot: &Shot,
    cameras: &mut Query<&mut CommanderCamera>,
    world_time: &mut Query<&mut WorldTime>,
) {
    for mut camera in cameras.iter_mut() {
        // Set BOTH the rendered value and the target. The commander camera eases
        // toward its targets every frame, so writing only the rendered value
        // would have the camera spring straight back to wherever the target
        // still pointed -- a capture harness that silently framed the wrong shot.
        camera.focus = shot.focus;
        camera.focus_target = shot.focus;
        camera.yaw = shot.yaw;
        camera.yaw_target = shot.yaw;
        camera.zoom = shot.zoom;
        camera.zoom_target = shot.zoom;
        camera.tilt = shot.tilt;
    }

    for mut time in world_time.iter_mut() {
        let cycle = time.day_duration + time.night_duration;
        time.seconds_in_cycle = normalized_to_seconds(shot.time_of_day, &time);
        time.ocean_seconds = shot.time_of_day * cycle;
    }
}

/// Invert `WorldTime::normalized_time()`.
///
/// That function maps the internal "day first, then night" layout onto a clock where
/// 0.0 = midnight, 0.25 = sunrise, **0.5 = noon**, 0.75 = sunset. Getting this backwards
/// silently photographs the world at dusk, which reads as "the renderer is broken".
fn normalized_to_seconds(normalized: f32, time: &WorldTime) -> f32 {
    // Delegate to the shared inverse so capture `--time` always agrees with
    // the game's display clock (now asymmetric summer hours, sunset 20:00) —
    // a hand-rolled copy here silently drifted once before.
    let mut scratch = time.clone();
    scratch.set_normalized_time(normalized);
    scratch.seconds_in_cycle
}

fn synthetic_settlement_history(name: &str) -> shared::economy::SettlementHistoryArchive {
    use shared::economy::{Good, MarketGoodHistoryDay, SettlementHistoryDay};

    let mut days = Vec::with_capacity(shared::economy::SETTLEMENT_HISTORY_DAYS);
    for day in 1..=shared::economy::SETTLEMENT_HISTORY_DAYS as u32 {
        let mut market = [MarketGoodHistoryDay::default(); Good::COUNT];
        let mut market_cash = [0u64; Good::COUNT];
        let mut physical_stock = [0u32; Good::COUNT];
        for good in Good::ALL {
            let wave =
                ((day as f32 * 0.071 + good.index() as f32).sin() * 0.18 + 1.0).clamp(0.6, 1.4);
            let midpoint = (good.base_price() as f32 * wave) as u64;
            let producer_units = if (day + good.index() as u32) % 3 == 0 {
                0
            } else {
                2 + u64::from(day % 5)
            };
            let consumer_units = 1 + u64::from((day + good.index() as u32) % 4);
            let producer_price = midpoint.saturating_mul(92) / 100;
            let consumer_price = midpoint.saturating_mul(108).div_ceil(100);
            let stock = 5 + ((day * (good.index() as u32 + 2)) % 24);
            let target = match good {
                Good::Food | Good::Wheat => 14,
                Good::Wood => 20,
                Good::Stone | Good::Iron => 5,
            };
            let cash = 2_000 + u64::from(day) * 9 + good.index() as u64 * 375;
            market[good.index()] = MarketGoodHistoryDay {
                opening_bid: producer_price.saturating_sub(3),
                opening_ask: consumer_price.saturating_sub(2),
                closing_bid: producer_price,
                closing_ask: consumer_price,
                high_bid: producer_price.saturating_add(8),
                low_bid: producer_price.saturating_sub(9),
                high_ask: consumer_price.saturating_add(10),
                low_ask: consumer_price.saturating_sub(7),
                producer_units,
                producer_coin: producer_price.saturating_mul(producer_units),
                consumer_units,
                consumer_coin: consumer_price.saturating_mul(consumer_units),
                closing_stock: stock,
                target_stock: target,
                pool_cash: cash,
            };
            market_cash[good.index()] = cash;
            physical_stock[good.index()] = stock + 3;
        }
        let population = 3 + day / 38;
        let employed = population.saturating_sub(if day % 47 < 8 { 2 } else { 1 });
        let hungry = u32::from(day % 53 < 5);
        let prosperity =
            (55.0 + day as f32 * 0.085 + (day as f32 * 0.12).sin() * 8.0).clamp(0.0, 100.0);
        let market_total = market_cash.iter().sum::<u64>();
        let resident_wallets = u64::from(population) * (900 + u64::from(day) * 4);
        let treasury = 2_000 + u64::from(day) * 12;
        let liquidation = Good::ALL
            .into_iter()
            .map(|good| u64::from(physical_stock[good.index()]) * market[good.index()].closing_bid)
            .sum();
        days.push(SettlementHistoryDay {
            day,
            market,
            civic_treasury: treasury,
            market_cash,
            resident_wallet_money: resident_wallets,
            pending_payments: if day % 11 == 0 { 125 } else { 0 },
            physical_stock,
            stock_liquidation_value: liquidation,
            total_local_coin: treasury + market_total + resident_wallets,
            population,
            employed,
            hungry,
            food_reserves: physical_stock[Good::Food.index()] + physical_stock[Good::Wheat.index()],
            food_produced: 3 + day % 7,
            food_consumed: population,
            buildings: (4 + day / 55) as u16,
            productive_buildings: (2 + day / 100) as u16,
            work_positions: (4 + day / 45) as u16,
            filled_jobs: employed.min(u16::MAX as u32) as u16,
            prosperity,
            reserve_prosperity: (prosperity * 0.38).min(40.0),
            production_prosperity: (prosperity * 0.29).min(30.0),
            housing_prosperity: (prosperity * 0.2).min(20.0),
            employment_prosperity: (prosperity * 0.1).min(10.0),
            hunger_penalty: -(hungry as f32 * 4.0),
        });
    }
    shared::economy::SettlementHistoryArchive {
        settlement: name.to_string(),
        days,
    }
}

fn synthetic_world_history() -> shared::economy::WorldHistoryArchive {
    use shared::economy::{Good, WorldHistoryDay};

    let days = (1..=shared::economy::SETTLEMENT_HISTORY_DAYS as u32)
        .map(|day| {
            let settlements = 2 + day / 90;
            let population = 18 + day / 8 + (day / 70) * 5;
            let employed = population.saturating_sub(3 + day % 4);
            let hungry = if day % 61 < 8 { 2 + day % 3 } else { day % 2 };
            let mut physical_stock = [0u32; Good::COUNT];
            for good in Good::ALL {
                physical_stock[good.index()] = 15 + day / 5 + good.index() as u32 * 9 + day % 13;
            }
            let market_cash = 28_000 + u64::from(day) * 37;
            let wallets = u64::from(population) * (850 + u64::from(day) * 3);
            let treasury = u64::from(settlements) * 2_500 + u64::from(day) * 18;
            WorldHistoryDay {
                day,
                settlements,
                population,
                employed,
                hungry,
                civic_treasury: treasury,
                market_cash,
                resident_wallet_money: wallets,
                pending_payments: if day % 17 == 0 { 220 } else { 0 },
                total_local_coin: treasury + market_cash + wallets,
                stock_liquidation_value: 18_000 + u64::from(day) * 91,
                physical_stock,
                food_reserves: physical_stock[Good::Food.index()]
                    + physical_stock[Good::Wheat.index()],
                food_produced: population + 5 + day % 12,
                food_consumed: population.saturating_sub(hungry),
                buildings: 8 + day / 17,
                productive_buildings: 4 + day / 43,
                prosperity: (48.0 + day as f32 * 0.1 + (day as f32 * 0.085).sin() * 6.0)
                    .clamp(0.0, 100.0),
            }
        })
        .collect();
    shared::economy::WorldHistoryArchive { days }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe() -> WorldTime {
        WorldTime {
            seconds_in_cycle: 0.0,
            day_duration: 600.0,
            night_duration: 300.0,
            ocean_seconds: 0.0,
            day: 0,
        }
    }

    /// Round-trip against the real accessor so a change to either side is caught.
    #[test]
    fn time_of_day_round_trips() {
        for target in [0.0_f32, 0.25, 0.5, 0.75, 0.9] {
            let mut t = probe();
            t.seconds_in_cycle = normalized_to_seconds(target, &t);
            let got = t.normalized_time();
            assert!(
                (got - target).abs() < 1e-3 || (got - target).abs() > 0.999,
                "time {target} round-tripped to {got}"
            );
        }
    }

    /// Display noon is NOT mid-day-portion since the summer clock (sunrise
    /// 06:00, sunset 22:00): it lands at the clock's fraction of daylight.
    #[test]
    fn noon_lands_at_the_summer_clock_fraction_of_daylight() {
        let t = probe();
        let frac = (0.5 - WorldTime::SUNRISE_NORMALIZED)
            / (WorldTime::SUNSET_NORMALIZED - WorldTime::SUNRISE_NORMALIZED);
        assert!((normalized_to_seconds(0.5, &t) - t.day_duration * frac).abs() < 1e-3);
    }
}
