//! Authored character, cargo, cart, boat and isolated-prop capture fixtures.

use super::CaptureConfig;
use bevy::prelude::*;

/// Render exactly one shipped prop on otherwise bare terrain.
///
/// `FISTFORCE_CAPTURE_PROP=<canonical PropKind id>` disables normal prop and
/// ground-cover streaming, waits for the real GLB mesh/material handles, and
/// stages its LOD0 mesh at the first shot's focus. This is deliberately more
/// literal than a forest survey: it answers whether the asset itself reaches
/// Bevy's render world before density, lighting or surrounding grass can hide
/// the result. `FISTFORCE_CAPTURE_PROP_SCALE` is inspection-only and defaults
/// to 1.0.
pub(super) fn spawn_capture_isolated_prop(
    mut commands: Commands,
    config: Res<CaptureConfig>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
    prop_assets: Option<Res<crate::props::PropAssets>>,
    meshes: Res<Assets<Mesh>>,
    materials: Res<Assets<StandardMaterial>>,
    world_root: Query<Entity, With<crate::render::systems::ClientWorldRoot>>,
    mut settings: ResMut<crate::render::systems::GraphicsSettings>,
    mut spawned: Local<bool>,
) {
    if *spawned {
        return;
    }
    let Ok(raw_kind) = std::env::var("FISTFORCE_CAPTURE_PROP") else {
        *spawned = true;
        return;
    };

    // Suppress the ordinary forest/grass streams immediately. The fixture is
    // intentionally not an EnvironmentProp, so their cleanup system cannot
    // remove the single specimen along with the surrounding scenery.
    settings.props_enabled = false;

    let Some(kind) = shared::props::PropKind::from_id(raw_kind.trim()) else {
        error!("capture: unknown FISTFORCE_CAPTURE_PROP={raw_kind:?}");
        *spawned = true;
        return;
    };
    let (Some(terrain), Some(prop_assets)) = (terrain, prop_assets) else {
        return;
    };
    let Some(mesh_set) = prop_assets.tree_meshes.get(&kind) else {
        error!(
            "capture: isolated prop {} has no direct mesh registration",
            kind.id()
        );
        *spawned = true;
        return;
    };
    if meshes.get(&mesh_set.lod0).is_none() || materials.get(&mesh_set.material).is_none() {
        return;
    }
    let Ok(world_root) = world_root.single() else {
        return;
    };
    let focus = config
        .shots
        .first()
        .map(|shot| shot.focus)
        .unwrap_or_default();
    let scale = std::env::var("FISTFORCE_CAPTURE_PROP_SCALE")
        .ok()
        .and_then(|raw| raw.parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(1.0);
    let position = Vec3::new(focus.x, terrain.get_height(focus.x, focus.z), focus.z);
    let specimen = commands
        .spawn((
            Name::new(format!("Capture specimen: {}", kind.id())),
            crate::props::PropKindTag(kind),
            crate::props::NeedsFoliageMaterials,
            Mesh3d(mesh_set.lod0.clone()),
            MeshMaterial3d(mesh_set.material.clone()),
            Transform::from_translation(position).with_scale(Vec3::splat(scale)),
            Visibility::Visible,
            InheritedVisibility::default(),
            bevy::light::NotShadowCaster,
        ))
        .id();
    commands.entity(world_root).add_child(specimen);
    info!(
        "capture: staged exactly one {} at ({:.1}, {:.1}) scale {:.2}",
        kind.id(),
        position.x,
        position.z,
        scale
    );
    *spawned = true;
}

/// `FISTFORCE_CAPTURE_DINGHY=underway|sailing|wreck` stages the real runtime
/// scene on water at the first shot's focus. This is presentation-only—the
/// live server remains the authority for navigation and disembarkation—but it
/// exercises the identical named sail nodes, morph and wreck state without a
/// login. `sailing` additionally advances the hull along its velocity so the
/// wake foam trail behind a genuinely moving boat can be photographed.
pub(super) fn spawn_capture_dinghy(
    mut commands: Commands,
    mut config: ResMut<CaptureConfig>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
    mut spawned: Local<bool>,
) {
    if *spawned {
        return;
    }
    let Ok(mode) = std::env::var("FISTFORCE_CAPTURE_DINGHY") else {
        *spawned = true;
        return;
    };
    let Some(terrain) = terrain else { return };
    let focus = config.shots.first().map_or(Vec3::ZERO, |shot| shot.focus);
    let Some(water) = terrain.get_water_height(focus.x, focus.z) else {
        error!(
            "capture: FISTFORCE_CAPTURE_DINGHY needs water at ({:.1}, {:.1})",
            focus.x, focus.z
        );
        *spawned = true;
        return;
    };
    let position = Vec3::new(focus.x, water, focus.z);
    // CLI captures commonly specify only X,Z and therefore default Y to zero.
    // Ocean height is map-authored, so that can put the review camera below
    // the surface. Keep every requested angle centred on the actual hull
    // waterline while preserving its XZ composition.
    for shot in config.shots.iter_mut() {
        shot.focus.y = water + 0.55;
    }
    let wrecked = mode.trim().eq_ignore_ascii_case("wreck");
    let sailing = mode.trim().eq_ignore_ascii_case("sailing");
    // Match the heading to the velocity so a sailing hull moves bow-first
    // (authored bow is -Z: yaw for velocity v is atan2(-v.x, -v.z)).
    let velocity = Vec3::new(2.0, 0.0, -1.0);
    let yaw = (-velocity.x).atan2(-velocity.z);
    let mut vessel = commands.spawn((
        shared::components::PlayerBoat,
        shared::components::Vessel,
        shared::components::CommandedBy("capture-sailor".into()),
        shared::components::PlayerPosition(position),
        shared::components::PlayerRotation(yaw),
        shared::components::CharacterMotion::new(velocity),
    ));
    if wrecked {
        vessel.insert(shared::components::WreckedVessel);
    }
    if sailing {
        vessel.insert(CaptureSailing);
    }
    if !wrecked {
        let manifest = match shared::character::CharacterManifest::load() {
            Ok(manifest) => manifest,
            Err(error) => {
                error!("capture: character manifest unavailable: {error}");
                *spawned = true;
                return;
            }
        };
        let helm = position + Quat::from_rotation_y(yaw) * Vec3::new(0.0, 0.35, 1.24);
        commands.spawn((
            shared::components::Hero {
                owner: lightyear::prelude::PeerId::Netcode(9_999),
            },
            shared::components::CharacterName("Capture Sailor".into()),
            shared::components::CharacterKind::Hero,
            shared::components::CharacterAffiliation::default(),
            shared::components::PersonId(99_999),
            shared::components::HeroOutfit::from_manifest(&manifest),
            shared::components::CommandedBy("capture-sailor".into()),
            shared::components::AboardBoat,
            shared::components::CharacterActivity::Sitting,
            shared::components::PlayerPosition(helm),
            shared::components::PlayerRotation(yaw),
            shared::components::CharacterMotion::new(velocity),
        ));
    }
    *spawned = true;
}

/// Marks the staged capture dinghy that should genuinely travel, so the wake
/// breadcrumb trail forms exactly as it does behind a live sailing boat.
#[derive(Component)]
pub(super) struct CaptureSailing;

pub(super) fn drive_capture_dinghy(
    time: Res<Time>,
    mut boats: Query<
        (
            &mut shared::components::PlayerPosition,
            &shared::components::CharacterMotion,
        ),
        With<CaptureSailing>,
    >,
) {
    for (mut position, motion) in boats.iter_mut() {
        position.0 += motion.velocity * time.delta_secs();
    }
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
/// `FISTFORCE_CAPTURE_HERO_OFFSET=x,z` offsets those heroes from the shot focus,
/// which is useful for verifying world-space UI such as the minimap marker.
/// `FISTFORCE_CAPTURE_PORTER_CART=0|1|2` gives that fixture an empty, half or
/// full cart while `FISTFORCE_CAPTURE_CARRIED` chooses its visible cargo.
pub(super) fn spawn_capture_heroes(
    mut commands: Commands,
    config: Res<CaptureConfig>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
    mut spawned: Local<bool>,
) {
    if *spawned {
        return;
    }
    let mut hero_spec = std::env::var("FISTFORCE_CAPTURE_HERO").ok();
    let hero_offset = std::env::var("FISTFORCE_CAPTURE_HERO_OFFSET")
        .ok()
        .and_then(|raw| {
            let mut parts = raw.split(',').map(|part| part.trim().parse::<f32>());
            match (parts.next(), parts.next(), parts.next()) {
                (Some(Ok(x)), Some(Ok(z)), None) if x.is_finite() && z.is_finite() => {
                    Some(Vec2::new(x, z))
                }
                _ => None,
            }
        })
        .unwrap_or(Vec2::ZERO);
    let villager_count = std::env::var("FISTFORCE_CAPTURE_VILLAGERS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok());
    let cart_slots = std::env::var("FISTFORCE_CAPTURE_PORTER_CART")
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .map(|slots| slots.min(2));
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
                    "mine" | "mining" | "quarrying" => {
                        Some(shared::components::CharacterActivity::Mining)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if hero_spec.is_none()
        && villager_count.is_none()
        && carried.is_empty()
        && activities.is_empty()
        && cart_slots.is_none()
    {
        *spawned = true;
        return;
    }
    if hero_spec.is_none()
        && (!carried.is_empty() || !activities.is_empty() || cart_slots.is_some())
    {
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
        let x = base.x + hero_offset.x + i as f32 * 1.4;
        let z = base.z + hero_offset.y;
        let pos = Vec3::new(x, terrain.get_height(x, z), z);
        let entity = commands
            .spawn((
                shared::components::Hero {
                    owner: lightyear::prelude::PeerId::Netcode(1000 + i as u64),
                },
                shared::components::CharacterName(format!("Capture Hero {}", i + 1)),
                shared::components::CharacterKind::Hero,
                shared::components::CharacterAffiliation::default(),
                shared::components::PersonId(10_000 + i as u64),
                outfit,
                shared::economy::Wallet::new(1_000),
                shared::components::PlayerPermitLedger::default(),
                shared::components::PlayerPosition(pos),
                shared::components::PlayerRotation(std::f32::consts::PI),
            ))
            .id();
        if let Some((good, appearance)) = carried.get(i % carried.len().max(1)).copied() {
            let amount = if cart_slots == Some(2) {
                shared::economy::capacity::PORTER / good.bulk_per_unit()
            } else {
                1
            };
            commands
                .entity(entity)
                .insert(shared::economy::CarriedLoad {
                    good: Some(good),
                    amount,
                    appearance: Some(appearance),
                });
        }
        if let Some(load_slots) = cart_slots {
            commands
                .entity(entity)
                .insert(shared::economy::PorterCartState { load_slots });
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
    } else if std::env::var("FISTFORCE_CAPTURE_SELECT").is_ok_and(|v| v == "building") {
        // Selects the first operating business so the compact card can be
        // photographed on a building rather than a settlement.
        commands.queue(|world: &mut World| {
            let entity = world
                .query_filtered::<Entity, (
                    With<shared::components::SettlementBuilding>,
                    With<shared::economy::BusinessAccount>,
                )>()
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

/// Reproducible offline fixture for the actual permit placement presentation.
///
/// `FISTFORCE_CAPTURE_PERMIT_PLACEMENT=farm|lumber|house` uses the real
/// placement systems, terrain and building assets. Only the server response is
/// staged; no separate mock ghost or mock quality calculation exists here.
pub(super) fn stage_capture_permit_placement(
    mut commands: Commands,
    config: Res<CaptureConfig>,
    settlements: Query<(
        &shared::components::SettlementId,
        &shared::components::PlayerPosition,
    )>,
    mut placement: ResMut<crate::hero::control::WorldPlacementMode>,
    mut staged: Local<bool>,
) {
    if *staged {
        return;
    }
    let Ok(raw) = std::env::var("FISTFORCE_CAPTURE_PERMIT_PLACEMENT") else {
        *staged = true;
        return;
    };
    let kind = match raw.trim().to_ascii_lowercase().as_str() {
        "farm" | "farmstead" => shared::components::SettlementBuildingKind::Farmstead,
        "lumber" | "lumberjack" => shared::components::SettlementBuildingKind::LumberjackHut,
        "house" | "cabin" => shared::components::SettlementBuildingKind::House,
        other => {
            warn!("capture: unknown permit-placement kind '{other}'");
            *staged = true;
            return;
        }
    };
    let Some((settlement_id, hall)) = settlements.iter().next() else {
        return;
    };
    let focus = config.shots.first().map_or(Vec3::ZERO, |shot| shot.focus);
    let cursor = Vec2::new(focus.x, focus.z);
    commands.insert_resource(crate::camera_rts::CursorTerrainOverride(cursor));
    let hall_door = shared::components::SettlementBuildingKind::Hall.entrance_position(hall.0, 0.0);
    commands.spawn((
        shared::components::VillageRoad {
            settlement: "Brackwater".into(),
            builder: "Capture Road Steward".into(),
            points: vec![Vec2::new(hall_door.x, hall_door.z), cursor],
            built_through: 2,
            width: 3.0,
            reserved_width: 4.0,
            surface: shared::components::RoadSurface::Dirt,
            class: shared::components::RoadClass::Lane,
            stone_committed: 0,
        },
        shared::components::RoadOf(*settlement_id),
    ));
    let permit = shared::components::PlayerPermit {
        id: shared::components::PermitId(900),
        settlement: *settlement_id,
        kind,
        fee_escrow: if kind == shared::components::SettlementBuildingKind::House {
            0
        } else {
            165
        },
        purchased_day: 1,
        company: None,
    };
    *placement = crate::hero::control::WorldPlacementMode::Permit {
        permit,
        settlement_name: "Brackwater".into(),
        rotation: 0.0,
    };
    *staged = true;
}
