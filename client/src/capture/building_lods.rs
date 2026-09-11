//! All authored variants, through the production mesh-swap path.
use crate::render::building_lod::{BuildingLod, BuildingLodOverride};
use bevy::prelude::*;

#[derive(Resource, Default)]
pub(super) struct BuildingLodReview {
    roots: Vec<Entity>,
    frames: u32,
}

#[derive(Resource)]
pub(super) struct ForcedBuildingLod(usize);

pub(super) fn install(app: &mut App) {
    if let Ok(value) = std::env::var("FISTFORCE_BUILDING_LOD") {
        let level = value
            .parse::<usize>()
            .expect("FISTFORCE_BUILDING_LOD must be 0, 1 or 2 (hidden)");
        assert!(
            level <= crate::render::building_lod::HIDDEN,
            "FISTFORCE_BUILDING_LOD must be 0, 1 or 2 (hidden)"
        );
        app.insert_resource(ForcedBuildingLod(level))
            .add_systems(Update, force);
    }
    if std::env::var("FISTFORCE_CAPTURE_BUILDING_LODS").as_deref() == Ok("1") {
        app.init_resource::<BuildingLodReview>()
            .add_systems(Update, stage);
    }
}

pub(super) fn ready(
    review: Option<Res<BuildingLodReview>>,
    forced: Option<Res<ForcedBuildingLod>>,
    roots: Query<&BuildingLod>,
) -> bool {
    forced.is_none_or(|forced| roots.iter().all(|lod| lod.ready && lod.level == forced.0))
        && review.is_none_or(|review| {
            !review.roots.is_empty()
                && review
                    .roots
                    .iter()
                    .all(|root| roots.get(*root).is_ok_and(|lod| lod.ready))
        })
}

fn force(
    mut commands: Commands,
    forced: Res<ForcedBuildingLod>,
    roots: Query<Entity, Added<BuildingLod>>,
    lods: Query<&BuildingLod>,
    mut loading_frames: Local<u32>,
) {
    if lods.iter().any(|lod| !lod.ready) {
        *loading_frames += 1;
        assert!(
            *loading_frames < 1200,
            "forced building LOD capture assets did not become ready"
        );
    } else {
        *loading_frames = 0;
    }
    for root in &roots {
        commands.entity(root).insert(BuildingLodOverride(forced.0));
    }
}

fn stage(
    mut commands: Commands,
    mut config: ResMut<super::CaptureConfig>,
    mut review: ResMut<BuildingLodReview>,
    mut terrain: ResMut<shared::terrain::WorldTerrain>,
    assets: Res<AssetServer>,
    mut settings: ResMut<crate::render::systems::GraphicsSettings>,
    roots: Query<Entity, With<crate::render::systems::ClientWorldRoot>>,
    lods: Query<&BuildingLod>,
) {
    review.frames += 1;
    assert!(
        review.frames < 1200
            || (!review.roots.is_empty()
                && review
                    .roots
                    .iter()
                    .all(|r| lods.get(*r).is_ok_and(|l| l.ready))),
        "building LOD assets did not become ready"
    );
    if !review.roots.is_empty() {
        return;
    }
    let Ok(root) = roots.single() else { return };
    settings.props_enabled = false;
    for (index, kind) in shared::building::BuildingType::all().iter().enumerate() {
        let x = (index % 4) as f32 * 60.0 - 270.0;
        let z = (index / 4) as f32 * 60.0 - 120.0;
        let at = Vec3::new(x, terrain.get_height(x, z), z);
        let def = kind.definition();
        terrain.apply_flatten_rect(
            at,
            def.terrain_flat_half_extents(),
            0.0,
            def.terrain_blend_width(),
        );
        let entity = commands.spawn((
            Name::new(format!("LOD review: {}", def.display_name)),
            WorldAssetRoot(assets.load(kind.scene_path().unwrap())),
            Transform::from_translation(at),
            Visibility::Inherited,
            ChildOf(root),
        ));
        review.roots.push(entity.id());
    }
    // Free-look shots otherwise use an absolute/sea-level eye. This dry-land
    // catalog has varying terrain heights; record the actual ground in each pose.
    for shot in &mut config.shots {
        if shot.pitch.is_some() {
            shot.focus.y = terrain.get_height(shot.focus.x, shot.focus.z);
        }
    }
}
