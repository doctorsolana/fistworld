//! Keep surviving scenery grounded when an edited terrain mesh commits.

use bevy::prelude::*;
use shared::terrain::WorldTerrain;

use super::{EnvironmentProp, PropChunkIndex};
use crate::terrain::TerrainChunk;

/// Terrain currently commits replacement entities. Observe that completion,
/// never WorldTerrain's earlier delta ingest or PBR's material-only mesh ticks.
/// The terrain finalize budget also bounds this work to a few chunk buckets.
pub(super) fn reground_props_after_terrain_commit(
    committed: Query<&TerrainChunk, Added<TerrainChunk>>,
    terrain: Res<WorldTerrain>,
    index: Res<PropChunkIndex>,
    mut props: Query<&mut Transform, With<EnvironmentProp>>,
) {
    for chunk in &committed {
        let Some(entities) = index.by_chunk.get(&chunk.coord) else {
            continue;
        };
        for &entity in entities {
            let Ok(mut transform) = props.get_mut(entity) else {
                continue;
            };
            // This is the same contract as spawn_prop_instance: authored prop
            // offsets live in the model, while its root rests at terrain height.
            let at = transform.translation;
            let height = terrain.get_height(at.x, at.z);
            if at.y != height {
                transform.translation.y = height;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::terrain::ChunkCoord;

    #[test]
    fn committed_earthworks_reground_local_roots_without_replacing_or_reshaping_them() {
        let mut app = App::new();
        app.init_resource::<WorldTerrain>()
            .init_resource::<PropChunkIndex>()
            .add_systems(Update, reground_props_after_terrain_commit);
        let positions = [Vec3::new(32.0, 0.0, 32.0), Vec3::new(224.0, 0.0, 32.0)];
        let mut fixtures = Vec::new();
        for position in positions {
            let height = app
                .world()
                .resource::<WorldTerrain>()
                .get_height(position.x, position.z);
            let transform = Transform::from_xyz(position.x, height, position.z)
                .with_rotation(Quat::from_rotation_y(0.73))
                .with_scale(Vec3::splat(1.4));
            let coord = ChunkCoord::from_world_pos(position);
            let entity = app
                .world_mut()
                .spawn((EnvironmentProp { chunk: coord }, transform))
                .id();
            app.world_mut()
                .resource_mut::<PropChunkIndex>()
                .by_chunk
                .entry(coord)
                .or_default()
                .push(entity);
            fixtures.push((entity, transform));
        }
        let (local, original) = fixtures[0];
        let (remote, remote_original) = fixtures[1];
        let target = original.translation + Vec3::Y * 1.5;
        app.world_mut()
            .resource_mut::<WorldTerrain>()
            .apply_flatten_rect(target, Vec2::splat(6.0), 0.0, 2.0);
        app.update();
        assert_eq!(
            *app.world().get::<Transform>(local).unwrap(),
            original,
            "keep old ground contact while the edited mesh is still building"
        );

        let terrain_entity = app
            .world_mut()
            .spawn(TerrainChunk {
                coord: ChunkCoord::from_world_pos(target),
                weightmap: default(),
                material: default(),
            })
            .id();
        app.update();
        let updated = *app.world().get::<Transform>(local).unwrap();
        let expected_height = app
            .world()
            .resource::<WorldTerrain>()
            .get_height(target.x, target.z);
        assert!((expected_height - original.translation.y).abs() > 1.0);
        assert_eq!(updated.translation.y, expected_height);
        assert_eq!(updated.translation.xz(), original.translation.xz());
        assert_eq!(updated.rotation, original.rotation);
        assert_eq!(updated.scale, original.scale);
        assert_eq!(
            *app.world().get::<Transform>(remote).unwrap(),
            remote_original
        );
        let mut roots = app
            .world_mut()
            .query_filtered::<Entity, With<EnvironmentProp>>();
        let mut surviving = roots.iter(app.world()).collect::<Vec<_>>();
        surviving.sort();
        let mut expected = vec![local, remote];
        expected.sort();
        assert_eq!(surviving, expected);

        // Ordinary retained component/material changes are not another commit.
        app.world_mut()
            .resource_mut::<WorldTerrain>()
            .apply_flatten_rect(target + Vec3::Y, Vec2::splat(6.0), 0.0, 2.0);
        app.world_mut()
            .get_mut::<TerrainChunk>(terrain_entity)
            .unwrap()
            .set_changed();
        app.update();
        assert_eq!(*app.world().get::<Transform>(local).unwrap(), updated);
    }
}
