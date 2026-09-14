//! Staged replicated house states through the real renderer. Paid construction
//! and the owner button are verified separately in the connected client lab.

use super::{CaptureConfig, CaptureState};
use bevy::prelude::*;
use shared::components::{
    BuildingDoorDemand, BuildingId, CharacterActivity, CharacterKind, CharacterName,
    ConstructionSite, HouseAppearance, HouseLevel, HouseLine, HouseUpgradeWorksite, Household,
    PersonId, PlayerPosition, PlayerRotation, SettlementBuilding, SettlementBuildingKind,
    HOUSE_UPGRADE_WOOD_REQUIRED,
};
use shared::economy::{Good, GoodsInventory};
use shared::terrain::WorldTerrain;

#[derive(Resource, Default)]
pub(super) struct UpgradeStudy {
    homes: Vec<Entity>,
    sites: Vec<Entity>,
    phase: Option<usize>,
    ready: bool,
    waiting: usize,
}

pub(super) fn install(app: &mut App) {
    if std::env::var("FISTFORCE_CAPTURE_HOUSE_UPGRADES").as_deref() != Ok("1") {
        return;
    }
    app.init_resource::<UpgradeStudy>()
        .add_systems(PostStartup, stage)
        .add_systems(PreUpdate, prepare)
        .add_systems(Last, inspect);
}

pub(super) fn ready(study: Option<Res<UpgradeStudy>>) -> bool {
    study.is_none_or(|study| study.ready)
}

fn stage(world: &mut World) {
    let focus = world.resource::<CaptureConfig>().shots[0].focus;
    for (index, line) in [HouseLine::Cabin, HouseLine::LongCabin]
        .into_iter()
        .enumerate()
    {
        let appearance = HouseAppearance {
            line,
            level: HouseLevel::Ground,
        };
        let mut at = focus + Vec3::new(if index == 0 { -7.0 } else { 7.0 }, 0.0, 0.0);
        let mut terrain = world.resource_mut::<WorldTerrain>();
        at.y = terrain.get_height(at.x, at.z);
        let definition = appearance.building_type().definition();
        terrain.apply_flatten_rect(
            at,
            definition.terrain_flat_half_extents(),
            0.0,
            definition.terrain_blend_width(),
        );
        let resident = format!("Extension resident {index}");
        world.spawn((
            CharacterName(resident.clone()),
            CharacterKind::Villager,
            CharacterActivity::Indoors,
        ));
        let home = world
            .spawn((
                BuildingId(980_000 + index as u64),
                SettlementBuilding {
                    kind: SettlementBuildingKind::House,
                    settlement: "House extension study".into(),
                    owner: Some(resident.clone()),
                    quality: 0.8,
                    workers: Vec::new(),
                },
                Household {
                    resident_ids: vec![PersonId(980_000 + index as u64)],
                    residents: vec![resident],
                },
                appearance,
                PlayerPosition(at),
                PlayerRotation(0.0),
                BuildingDoorDemand { open: false },
            ))
            .id();
        world.resource_mut::<UpgradeStudy>().homes.push(home);
    }
}

fn prepare(world: &mut World) {
    let phase = match *world.resource::<CaptureState>() {
        CaptureState::Warmup { .. } => {
            let shot = &world.resource::<CaptureConfig>().shots[0];
            let (focus, yaw, zoom) = (shot.focus, shot.yaw, shot.zoom);
            for mut camera in world
                .query::<&mut crate::camera_rts::CommanderCamera>()
                .iter_mut(world)
            {
                camera.focus = focus;
                camera.focus_target = focus;
                camera.yaw = yaw;
                camera.yaw_target = yaw;
                camera.zoom = zoom;
                camera.zoom_target = zoom;
            }
            0
        }
        CaptureState::Settling { shot, .. } => shot,
        _ => return,
    };
    if world.resource::<UpgradeStudy>().phase == Some(phase) {
        return;
    }
    let homes = world.resource::<UpgradeStudy>().homes.clone();
    match phase {
        1 => {
            for home in homes {
                let id = *world.get::<BuildingId>(home).unwrap();
                let at = world.get::<PlayerPosition>(home).unwrap().0;
                let mut target = *world.get::<HouseAppearance>(home).unwrap();
                target.level = HouseLevel::UpperStorey;
                let mut stock = GoodsInventory::new(100);
                stock.add(Good::Wood, 4);
                let site = world
                    .spawn((
                        HouseUpgradeWorksite {
                            house: id,
                            owner: PersonId(id.0),
                            target,
                            wood_required: HOUSE_UPGRADE_WOOD_REQUIRED,
                        },
                        ConstructionSite {
                            kind: SettlementBuildingKind::House,
                            settlement: "House extension study".into(),
                            raising: false,
                            stand: at + Vec3::new(0.0, 0.0, 6.0),
                            rotation: 0.0,
                        },
                        PlayerPosition(at),
                        stock,
                    ))
                    .id();
                world.resource_mut::<UpgradeStudy>().sites.push(site);
            }
        }
        2 => {
            let sites = world.resource::<UpgradeStudy>().sites.clone();
            for site in sites {
                world.get_mut::<ConstructionSite>(site).unwrap().raising = true;
                world
                    .get_mut::<GoodsInventory>(site)
                    .unwrap()
                    .add(Good::Wood, 4);
            }
            for home in homes {
                world.get_mut::<BuildingDoorDemand>(home).unwrap().open = true;
            }
        }
        3 => {
            let sites = std::mem::take(&mut world.resource_mut::<UpgradeStudy>().sites);
            for site in sites {
                world.despawn(site);
            }
            for home in homes {
                world.get_mut::<HouseAppearance>(home).unwrap().level = HouseLevel::UpperStorey;
            }
        }
        _ => {}
    }
    let mut study = world.resource_mut::<UpgradeStudy>();
    study.phase = Some(phase);
    study.ready = false;
    study.waiting = 0;
}

fn inspect(world: &mut World) {
    let study = world.resource::<UpgradeStudy>();
    if study.ready || study.phase.is_none() {
        return;
    }
    let phase = study.phase.unwrap();
    let homes = study.homes.clone();
    let sites = study.sites.clone();
    let ready = homes.iter().all(|home| {
        let expected = world.get::<HouseAppearance>(*home).unwrap().building_type();
        world
            .get::<crate::settlement::BuildingVisual>(*home)
            .is_some_and(|visual| visual.rendered_type() == expected)
            && world
                .get::<crate::render::building_lod::BuildingLod>(*home)
                .is_some_and(|lod| lod.ready)
            && world
                .get::<shared::building::PlacedBuilding>(*home)
                .is_some_and(|placed| placed.building_type == expected)
    }) && sites.iter().all(|site| {
        world
            .get::<Children>(*site)
            .is_some_and(|children| !children.is_empty())
    });
    let mut study = world.resource_mut::<UpgradeStudy>();
    study.waiting += 1;
    assert!(
        study.waiting < 1800,
        "house upgrade presentation did not become ready"
    );
    if !ready || study.waiting < 30 {
        return;
    }
    study.ready = true;
    let homes: Vec<_> = homes
        .iter()
        .map(|home| {
            serde_json::json!({
                "entity": home.to_bits(), "id": world.get::<BuildingId>(*home),
                "appearance": world.get::<HouseAppearance>(*home),
                "capacity": world.get::<HouseAppearance>(*home).unwrap().level.housing_capacity(),
                "household": world.get::<Household>(*home),
                "door": world.get::<BuildingDoorDemand>(*home),
            })
        })
        .collect();
    let sites: Vec<_> = sites.iter().map(|site| {
        assert!(world.get::<crate::settlement::BuildingVisual>(*site).is_none(), "an extension must not render a second house");
        assert!(world.get::<shared::building::PlacedBuilding>(*site).is_none(), "an extension must not claim another footprint");
        let visible_wood = world.get::<Children>(*site).unwrap().iter().filter(|child| {
            world.get::<Name>(*child).is_some_and(|name| name.as_str().starts_with("Delivered Wood "))
                && world.get::<Visibility>(*child) == Some(&Visibility::Inherited)
        }).count();
        assert_eq!(visible_wood, world.get::<GoodsInventory>(*site).unwrap().amount(Good::Wood) as usize, "recoverable worksite Wood must remain visible");
        serde_json::json!({"visible_wood_bundles":visible_wood, "entity": site.to_bits(), "work": world.get::<HouseUpgradeWorksite>(*site), "site": world.get::<ConstructionSite>(*site), "inventory": world.get::<GoodsInventory>(*site)})
    }).collect();
    let out = world
        .resource::<CaptureConfig>()
        .out_dir
        .join(format!("phase-{phase}.json"));
    std::fs::write(out, serde_json::to_vec_pretty(&serde_json::json!({"scope":"offline presentation fixture; no simulated spending or building work", "homes":homes,"sites":sites})).unwrap()).unwrap();
}
