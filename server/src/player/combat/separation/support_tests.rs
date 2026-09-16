use super::*;
use crate::world::bridges::{BridgeDecks, rebuild_bridge_decks};
use shared::components::{PortGeometry, RoadBridge, SettlementId, SettlementPort, ShipKind};
use shared::terrain::WorldTerrain;

fn terrain() -> (WorldTerrain, f32) {
    let mut terrain = WorldTerrain::default();
    let water = terrain
        .water_surface_height(0.0, 0.0)
        .expect("water-capable map");
    terrain.apply_flatten_rect(
        Vec3::new(0.0, water + 1.0, 0.0),
        Vec2::splat(64.0),
        0.0,
        0.0,
    );
    terrain.apply_flatten_rect(
        Vec3::new(0.0, water - 6.0, 0.0),
        Vec2::new(12.0, 48.0),
        0.0,
        0.0,
    );
    assert!(terrain.get_water_height(0.0, 0.0).is_some());
    (terrain, water)
}

fn app(terrain: WorldTerrain) -> App {
    let mut app = App::new();
    app.insert_resource(terrain)
        .init_resource::<BridgeDecks>()
        .add_systems(
            Update,
            (rebuild_bridge_decks, separate_melee_bodies).chain(),
        );
    app
}

fn pair(app: &mut App, center: Vec3, kind: CharacterKind) -> [Entity; 2] {
    [-0.05, 0.05].map(|offset| {
        let point = center + Vec3::X * offset;
        app.world_mut()
            .spawn((
                kind,
                PlayerPosition(point),
                shared::region::RegionCoord::from_world_pos(point),
                CommandedBy("support test".into()),
            ))
            .id()
    })
}

fn positions(app: &App, pair: [Entity; 2]) -> [Vec3; 2] {
    pair.map(|entity| app.world().get::<PlayerPosition>(entity).unwrap().0)
}

fn bridge(water: f32, built: bool) -> RoadBridge {
    RoadBridge {
        start: Vec3::new(-20.0, water + 1.0, 0.0),
        end: Vec3::new(20.0, water + 1.0, 0.0),
        deck_height: water + 4.0,
        ramp_length: 8.0,
        width: 3.6,
        built,
    }
}

#[test]
fn melee_separation_keeps_fighters_on_completed_bridge_and_port_decks() {
    for port in [false, true] {
        for (kind, mounted) in [
            (CharacterKind::Villager, false),
            (CharacterKind::Hero, false),
            (CharacterKind::Villager, true),
        ] {
            let (terrain, water) = terrain();
            let mut app = app(terrain);
            let center = if port {
                let geometry = PortGeometry {
                    shore: Vec3::new(-26.0, water + 1.0, 0.0),
                    pier_end: Vec3::new(0.0, water + 1.4, 0.0),
                    berth: Vec3::new(4.0, water, 0.0),
                    departure: Vec3::new(4.0, water, -12.0),
                    yaw: 0.0,
                    maximum_ship: ShipKind::Coaster,
                };
                assert!(geometry.valid());
                app.world_mut().spawn(SettlementPort {
                    settlement: SettlementId(1),
                    geometry,
                    built: true,
                });
                geometry.deck_point(24.0)
            } else {
                let deck = bridge(water, true);
                assert!(deck.valid());
                let center = Vec3::new(0.0, deck.deck_height, 0.0);
                app.world_mut().spawn(deck);
                center
            };
            let fighters = pair(&mut app, center, kind);
            if mounted {
                for fighter in fighters {
                    app.world_mut()
                        .entity_mut(fighter)
                        .insert(shared::components::Mounted {
                            horse: fighter.to_bits(),
                            gait: shared::components::HorseGait::Walk,
                            phase: shared::components::RidingPhase::Riding,
                            since: 0.0,
                        });
                }
            }
            for _ in 0..8 {
                app.update();
            }
            let [a, b] = positions(&app, fighters);
            assert!(
                a.x < center.x - 0.2 && b.x > center.x + 0.2,
                "supported fighters did not separate"
            );
            for at in [a, b] {
                let clearance = if mounted {
                    shared::components::HORSE_CLEARANCE
                } else {
                    shared::physics::CHARACTER_NAV_RADIUS
                };
                let surface = app
                    .world()
                    .resource::<BridgeDecks>()
                    .height_at(at.xz(), clearance)
                    .unwrap();
                assert!(
                    (at.y - surface).abs() < 1.0e-4,
                    "separation dropped a fighter below the deck: {at:?}"
                );
                assert!(
                    app.world()
                        .resource::<WorldTerrain>()
                        .get_water_height(at.x, at.z)
                        .is_some()
                );
            }
        }
    }
}

#[test]
fn absent_deck_or_a_body_below_it_cannot_gain_support_from_separation() {
    for built in [false, true] {
        let (terrain, water) = terrain();
        let mut app = app(terrain);
        let deck = app.world_mut().spawn(bridge(water, built)).id();
        let center = Vec3::new(0.0, if built { water } else { water + 4.0 }, 0.0);
        let fighters = pair(&mut app, center, CharacterKind::Hero);
        let before = positions(&app, fighters);
        app.update();
        assert_eq!(
            positions(&app, fighters),
            before,
            "an unfinished crossing or an overhead deck cannot authorize displacement"
        );
        if built {
            for fighter in fighters {
                app.world_mut()
                    .get_mut::<PlayerPosition>(fighter)
                    .unwrap()
                    .0
                    .y = water + 4.0;
            }
            app.world_mut().despawn(deck);
            let before_removal = positions(&app, fighters);
            app.update();
            assert_eq!(
                positions(&app, fighters),
                before_removal,
                "removed crossing still authorized a correction"
            );
        }
    }
}

#[test]
fn shoreline_separation_does_not_push_walkers_into_unsupported_water() {
    for kind in [CharacterKind::Villager, CharacterKind::Hero] {
        let (terrain, _) = terrain();
        // Find the actual interpolated bank boundary rather than assuming a
        // terrain sample spacing or authoring a dry body at a wet coordinate.
        let edge = (0..4000)
            .map(|i| -20.0 + i as f32 * 0.005)
            .find(|x| terrain.get_water_height(*x, 0.0).is_some())
            .unwrap();
        let outer_x = edge - 0.02;
        assert!(terrain.get_water_height(outer_x, 0.0).is_none());
        assert!(
            terrain
                .get_water_height(outer_x + MAX_PUSH_PER_TICK, 0.0)
                .is_some()
        );
        let outer_y = terrain.get_height(outer_x, 0.0);
        let inner_y = terrain.get_height(outer_x - 0.1, 0.0);
        let mut app = app(terrain);
        let fighters = pair(&mut app, Vec3::new(outer_x - 0.05, outer_y, 0.0), kind);
        app.world_mut()
            .get_mut::<PlayerPosition>(fighters[0])
            .unwrap()
            .0
            .y = inner_y;
        let outer_before = app.world().get::<PlayerPosition>(fighters[1]).unwrap().0;
        app.update();
        assert_eq!(
            app.world().get::<PlayerPosition>(fighters[1]).unwrap().0,
            outer_before,
            "combat correction pushed a walker off the bank"
        );
    }
}
