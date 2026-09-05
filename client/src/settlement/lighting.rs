//! House window glow, bounded local lamps and building night lighting.

use super::buildings::BuildingVisual;
use super::grounds::FishingPierVisual;
use bevy::gltf::GltfMaterialName;
use bevy::light::NotShadowCaster;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use shared::components::{
    Household, PlayerPosition, SettlementBuilding, SettlementBuildingKind, WorldTime,
};

/// The cabin art already separates its panes and provides light anchors. This
/// component records the per-house material clone and lamps after that scene is
/// instantiated, so one occupied cabin can glow without modifying every house
/// that shares the source glTF material.
#[derive(Component)]
pub(super) struct HouseWindowLighting {
    pub(super) glass: Handle<StandardMaterial>,
    pub(super) lamps: Vec<Entity>,
    pub(super) strength: f32,
    pub(super) lamp_strength: f32,
}

#[derive(Default)]
pub(super) struct HouseLightBudget {
    pub(super) candidates: Vec<(f32, Entity)>,
    pub(super) enabled: HashSet<Entity>,
    pub(super) last_update_seconds: f64,
    pub(super) initialized: bool,
}

#[derive(Component)]
pub(super) struct HouseWindowLamp;

#[derive(Component)]
pub(super) struct BuildingNightLighting {
    pub(super) lamps: Vec<Entity>,
    pub(super) strength: f32,
}

#[derive(Component)]
pub(super) struct BuildingNightLamp {
    pub(super) lumens: f32,
}

pub(super) const CABIN_GLASS_MATERIAL: &str = "CabinGlass";

pub(super) const WINDOW_LIGHT_ANCHORS: [&str; 2] = ["Light_Window.L", "Light_Window.R"];

// The world camera uses physical sunlight exposure. This is deliberately much
// brighter than a literal domestic bulb, but its tight range keeps it a soft
// pool beneath the window instead of an orange floodlight across the village.
pub(super) const WINDOW_LIGHT_LUMENS: f32 = 950_000.0;

pub(super) const WINDOW_LIGHT_RANGE: f32 = 6.0;

pub(super) const WINDOW_FADE_PER_SECOND: f32 = 1.5;

pub(super) const WINDOW_EMISSIVE: LinearRgba = LinearRgba::new(13.0, 4.25, 0.80, 1.0);

/// Emissive panes sell a whole living town cheaply. Real clustered point
/// lights are reserved for the close neighbourhood where their ground spill
/// is visible; hundreds of distant domestic lights only burden light
/// preparation and fragment shading.
pub(super) const MAX_ACTIVE_HOUSE_POINT_LIGHTS: usize = 40;

pub(super) const HOUSE_POINT_LIGHT_RADIUS: f32 = 190.0;

pub(super) const HOUSE_LIGHT_BUDGET_INTERVAL_SECONDS: f64 = 0.25;

/// Wire one completed cabin to the glass and light anchors authored in its GLB.
///
/// Scene instantiation is asynchronous, so this polls only unwired houses. Once
/// all panes and both anchors exist it clones the glass material for that one
/// cabin, attaches a small warm light to each anchor, and stops scanning it.
pub(super) fn setup_house_window_lighting(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    houses: Query<
        Entity,
        (
            With<SettlementBuilding>,
            With<Household>,
            With<BuildingVisual>,
            Without<HouseWindowLighting>,
        ),
    >,
    children: Query<&Children>,
    names: Query<&Name>,
    primitives: Query<(&GltfMaterialName, &MeshMaterial3d<StandardMaterial>)>,
) {
    for house in houses.iter() {
        let mut stack = vec![house];
        let mut panes = Vec::new();
        let mut anchors: HashMap<&'static str, Entity> = HashMap::new();

        while let Some(entity) = stack.pop() {
            if let Ok((material_name, material)) = primitives.get(entity) {
                if material_name.0 == CABIN_GLASS_MATERIAL {
                    panes.push((entity, material.0.clone()));
                }
            }
            if let Ok(name) = names.get(entity) {
                for wanted in WINDOW_LIGHT_ANCHORS {
                    if name.as_str() == wanted {
                        anchors.insert(wanted, entity);
                    }
                }
            }
            if let Ok(entity_children) = children.get(entity) {
                stack.extend(entity_children.iter());
            }
        }

        if panes.is_empty() || anchors.len() != WINDOW_LIGHT_ANCHORS.len() {
            // The scene is still loading. Retry next frame rather than falling
            // back to offsets that will drift when the art changes.
            continue;
        }
        let Some(mut glass) = materials.get(&panes[0].1).cloned() else {
            continue;
        };
        glass.emissive = LinearRgba::BLACK;
        // A stable screen-space glow is more legible than forcing the pane to
        // compete with Exposure::SUNLIGHT. The point lights still use physical
        // units and provide the restrained spill on nearby ground and timber.
        glass.emissive_exposure_weight = 0.0;
        let glass = materials.add(glass);
        for (pane, _) in panes {
            commands.entity(pane).insert((
                MeshMaterial3d(glass.clone()),
                // The glass is the luminous surface, not an occluder sitting
                // between the lamp and the exterior pool it is meant to sell.
                NotShadowCaster,
            ));
        }

        let mut lamps = Vec::with_capacity(WINDOW_LIGHT_ANCHORS.len());
        for anchor_name in WINDOW_LIGHT_ANCHORS {
            let anchor = anchors[anchor_name];
            commands.entity(anchor).with_children(|parent| {
                lamps.push(
                    parent
                        .spawn((
                            Name::new(format!("Cabin glow at {anchor_name}")),
                            HouseWindowLamp,
                            PointLight {
                                color: Color::srgb(1.0, 0.53, 0.20),
                                intensity: 0.0,
                                range: WINDOW_LIGHT_RANGE,
                                radius: 0.32,
                                shadow_maps_enabled: false,
                                ..default()
                            },
                            Transform::default(),
                            Visibility::Hidden,
                        ))
                        .id(),
                );
            });
        }
        commands.entity(house).insert(HouseWindowLighting {
            glass,
            lamps,
            strength: 0.0,
            lamp_strength: 0.0,
        });
    }
}

/// Fade occupied cabins on after dark and back off at dawn or when empty.
///
/// The replicated household on the cabin is the durable occupancy fact. Do not
/// cross-join it with separate character entities here: those entities may be
/// streamed or replicated a frame later than the building's regional detail,
/// which used to leave a genuinely occupied cabin dark in live play.
pub(super) fn sync_house_window_lighting(
    mut commands: Commands,
    time: Res<Time>,
    world_time: Query<&WorldTime>,
    camera: Query<&crate::camera_rts::CommanderCamera>,
    mut houses: Query<(
        Entity,
        &Household,
        Option<&PlayerPosition>,
        &mut HouseWindowLighting,
    )>,
    mut lamps: Query<(&mut PointLight, &mut Visibility), With<HouseWindowLamp>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut budget: Local<HouseLightBudget>,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };

    let now = time.elapsed_secs_f64();
    if !budget.initialized
        || now - budget.last_update_seconds >= HOUSE_LIGHT_BUDGET_INTERVAL_SECONDS
    {
        budget.initialized = true;
        budget.last_update_seconds = now;
        budget.candidates.clear();
        budget.enabled.clear();

        if let Ok(camera) = camera.single() {
            if camera.zoom <= 420.0 {
                for (house, household, position, _) in houses.iter() {
                    if household.residents.is_empty() {
                        continue;
                    }
                    let Some(position) = position else { continue };
                    let distance_squared =
                        Vec2::new(position.0.x - camera.focus.x, position.0.z - camera.focus.z)
                            .length_squared();
                    if distance_squared <= HOUSE_POINT_LIGHT_RADIUS * HOUSE_POINT_LIGHT_RADIUS {
                        budget.candidates.push((distance_squared, house));
                    }
                }
                budget
                    .candidates
                    .sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
                let count = budget.candidates.len().min(MAX_ACTIVE_HOUSE_POINT_LIGHTS);
                for index in 0..count {
                    let house = budget.candidates[index].1;
                    budget.enabled.insert(house);
                }
            }
        } else {
            // Headless visual tests and unusual camera-less transitions contain
            // only a handful of houses; preserve the straightforward behavior.
            for (house, household, _, _) in houses.iter() {
                if !household.residents.is_empty() {
                    budget.enabled.insert(house);
                }
            }
        }
    }

    for (house, household, _position, mut window) in houses.iter_mut() {
        // Scene streaming/re-instantiation can replace a house's descendants
        // while the replicated building entity survives. The old component
        // then points at despawned lamps and used to remain permanently dark
        // because a stable strength took the early-return below. Drop the
        // stale wiring so setup discovers the new panes and anchors next frame.
        let wiring_is_live = materials.get(&window.glass).is_some()
            && window.lamps.iter().all(|lamp| lamps.get(*lamp).is_ok());
        if !wiring_is_live {
            commands.entity(house).remove::<HouseWindowLighting>();
            continue;
        }
        let occupied = !household.residents.is_empty();
        let target = house_window_target(clock, occupied);
        let next_glow = move_towards(
            window.strength,
            target,
            time.delta_secs() * WINDOW_FADE_PER_SECOND,
        );
        let lamp_target = if budget.enabled.contains(&house) {
            target
        } else {
            0.0
        };
        let next_lamp = move_towards(
            window.lamp_strength,
            lamp_target,
            time.delta_secs() * WINDOW_FADE_PER_SECOND,
        );

        if (next_glow - window.strength).abs() > f32::EPSILON {
            window.strength = next_glow;
            if let Some(mut glass) = materials.get_mut(&window.glass) {
                glass.emissive = WINDOW_EMISSIVE * next_glow;
            }
        }
        if (next_lamp - window.lamp_strength).abs() > f32::EPSILON {
            window.lamp_strength = next_lamp;
            for lamp in &window.lamps {
                if let Ok((mut light, mut visibility)) = lamps.get_mut(*lamp) {
                    light.intensity = WINDOW_LIGHT_LUMENS * next_lamp;
                    *visibility = if next_lamp > 0.001 {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    };
                }
            }
        }
    }
}

/// Turn art-authored workplace and pier anchors into restrained warm night
/// lights. The anchors remain the source of placement truth, so art can move a
/// lantern or hearth without a matching code edit.
pub(super) fn setup_building_night_lighting(
    mut commands: Commands,
    roots: Query<
        Entity,
        (
            Or<(With<BuildingVisual>, With<FishingPierVisual>)>,
            Without<BuildingNightLighting>,
        ),
    >,
    buildings: Query<&SettlementBuilding>,
    pier_roots: Query<(), With<FishingPierVisual>>,
    children: Query<&Children>,
    names: Query<&Name>,
) {
    for root in roots.iter() {
        let building_kind = buildings.get(root).ok().map(|building| building.kind);
        let light_specs: &[(&str, f32, f32)] = if pier_roots.get(root).is_ok() {
            // This world uses Exposure::SUNLIGHT; physical point lights must
            // be in the same calibrated range as occupied cabin lamps.
            &[("Light_Lantern", 520_000.0, 8.5)]
        } else {
            match building_kind {
                Some(SettlementBuildingKind::FishermansHut) => {
                    &[("Light_Interior", 820_000.0, 10.0)]
                }
                // The barn is not a Household, so the cabin window-glow path skips it; one warm
                // interior lamp at the authored anchor is what says "someone is in with the flock".
                Some(SettlementBuildingKind::LivestockFarm) => {
                    &[("Light_Interior", 720_000.0, 9.0)]
                }
                Some(SettlementBuildingKind::Windmill | SettlementBuildingKind::Bakery) => &[
                    ("Light_Interior", 720_000.0, 9.0),
                    ("Light_Lantern", 440_000.0, 7.5),
                ],
                Some(SettlementBuildingKind::Market) => &[
                    // The open square wants pools of warmth rather than one
                    // building-sized floodlight. Both authored levels expose
                    // these same anchors, so promotion keeps the composition.
                    ("Light_Interior", 760_000.0, 11.0),
                    ("Light_Lantern", 460_000.0, 8.0),
                ],
                _ => continue,
            }
        };

        let mut anchors = HashMap::new();
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            if let Ok(name) = names.get(entity) {
                for (wanted, _, _) in light_specs {
                    if name.as_str() == *wanted {
                        anchors.insert(*wanted, entity);
                    }
                }
            }
            if let Ok(entity_children) = children.get(entity) {
                stack.extend(entity_children.iter());
            }
        }
        if anchors.len() != light_specs.len() {
            // The glTF scene is still loading.
            continue;
        }

        let mut lamps = Vec::with_capacity(light_specs.len());
        for (anchor_name, lumens, range) in light_specs {
            let anchor = anchors[anchor_name];
            commands.entity(anchor).with_children(|parent| {
                lamps.push(
                    parent
                        .spawn((
                            Name::new(format!("Night glow at {anchor_name}")),
                            BuildingNightLamp { lumens: *lumens },
                            PointLight {
                                color: Color::srgb(1.0, 0.49, 0.18),
                                intensity: 0.0,
                                range: *range,
                                radius: 0.28,
                                shadow_maps_enabled: false,
                                ..default()
                            },
                            Transform::default(),
                            Visibility::Hidden,
                        ))
                        .id(),
                );
            });
        }
        commands.entity(root).insert(BuildingNightLighting {
            lamps,
            strength: 0.0,
        });
    }
}

pub(super) fn sync_building_night_lighting(
    mut commands: Commands,
    time: Res<Time>,
    world_time: Query<&WorldTime>,
    mut roots: Query<(Entity, &mut BuildingNightLighting)>,
    mut lamps: Query<(&BuildingNightLamp, &mut PointLight, &mut Visibility)>,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let target = house_window_target(clock, true);
    for (root, mut lighting) in roots.iter_mut() {
        // Replacing a level's WorldAssetRoot despawns its old anchor children.
        // Drop stale wiring even if the fade target has not changed, allowing
        // setup to bind the new market scene on the following frame.
        if lighting
            .lamps
            .iter()
            .any(|lamp| lamps.get_mut(*lamp).is_err())
        {
            commands.entity(root).remove::<BuildingNightLighting>();
            continue;
        }
        let next = move_towards(
            lighting.strength,
            target,
            time.delta_secs() * WINDOW_FADE_PER_SECOND,
        );
        if (next - lighting.strength).abs() <= f32::EPSILON {
            continue;
        }
        lighting.strength = next;
        for lamp in &lighting.lamps {
            if let Ok((lamp, mut light, mut visibility)) = lamps.get_mut(*lamp) {
                light.intensity = lamp.lumens * next;
                *visibility = if next > 0.001 {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        }
    }
}

pub(super) fn house_window_target(clock: &WorldTime, occupied: bool) -> f32 {
    if !occupied {
        return 0.0;
    }
    // Begin after the sun clears the horizon and reach full warmth in early
    // twilight. This avoids orange windows fighting broad daylight while still
    // making a newly occupied cabin feel alive before the sky is fully black.
    let elevation = -clock.sun_phase().cos();
    let t = ((-elevation - 0.02) / 0.16).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub(super) fn move_towards(current: f32, target: f32, max_delta: f32) -> f32 {
    if (target - current).abs() <= max_delta {
        target
    } else {
        current + (target - current).signum() * max_delta
    }
}
