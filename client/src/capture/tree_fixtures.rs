//! The meadow palette, both actual LODs, through production foliage materials.
use super::CaptureConfig;
use bevy::prelude::*;

pub(super) fn spawn_tree_review(
    mut commands: Commands,
    config: Res<CaptureConfig>,
    terrain: Res<shared::terrain::WorldTerrain>,
    assets: Res<crate::props::PropAssets>,
    meshes: Res<Assets<Mesh>>,
    materials: Res<Assets<StandardMaterial>>,
    roots: Query<Entity, With<crate::render::systems::ClientWorldRoot>>,
    mut settings: ResMut<crate::render::systems::GraphicsSettings>,
    mut spawned: Local<bool>,
) {
    let mode = std::env::var("FISTFORCE_CAPTURE_TREES").unwrap_or_default();
    if *spawned || !matches!(mode.as_str(), "lineup" | "trio") {
        return;
    }
    settings.props_enabled = false;
    use shared::props::PropKind::*;
    let kinds: &[shared::props::PropKind] = if mode == "trio" {
        &[FieldMapleA, CopperBeechA, WildCherryA]
    } else {
        &[FieldMapleA, CopperBeechA, WildCherryA, OakA, PineA]
    };
    // Wait for both real LOD meshes and their material, not a fixed delay.
    for kind in kinds {
        let Some(set) = assets.tree_meshes.get(kind) else {
            return;
        };
        if !meshes.contains(&set.lod0)
            || set.lod1.as_ref().is_none_or(|h| !meshes.contains(h))
            || !materials.contains(&set.material)
        {
            return;
        }
    }
    let Ok(root) = roots.single() else { return };
    let focus = config.shots.first().map_or(Vec3::ZERO, |s| s.focus);
    for (column, &kind) in kinds.iter().enumerate() {
        let set = &assets.tree_meshes[&kind];
        for (row, mesh) in [set.lod0.clone(), set.lod1.clone().unwrap()]
            .into_iter()
            .take(if mode == "trio" { 1 } else { 2 })
            .enumerate()
        {
            let x = focus.x + (column as f32 - (kinds.len() - 1) as f32 * 0.5) * 9.0;
            let z = focus.z
                + if mode == "trio" {
                    0.0
                } else {
                    (row as f32 - 0.5) * 13.0
                };
            let specimen = commands
                .spawn((
                    Name::new(format!("Tree review: {} LOD{row}", kind.id())),
                    crate::props::PropKindTag(kind),
                    crate::props::NeedsFoliageMaterials,
                    Mesh3d(mesh),
                    MeshMaterial3d(set.material.clone()),
                    Transform::from_xyz(x, terrain.get_height(x, z), z),
                    Visibility::Visible,
                ))
                .id();
            commands.entity(root).add_child(specimen);
        }
    }
    info!("capture: meadow palette ready ({mode})");
    *spawned = true;
}
