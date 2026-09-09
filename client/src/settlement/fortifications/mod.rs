//! Region-scoped presentation of authoritative civic defense sections.

mod mesh;

use bevy::prelude::*;
use shared::components::{FortificationKind, FortificationMaterial, FortificationSegment};

use crate::states::GameState;

pub(super) struct FortificationPlugin;

impl Plugin for FortificationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, attach_sections.run_if(in_state(GameState::Playing)));
    }
}

#[derive(Component)]
pub(crate) struct FortificationVisual {
    state: FortificationSegment,
    supplied: bool,
}

/// Rebuilding a whole newly observed circuit in one frame can stall loading.
/// Leave pending roots unmarked and prepare at most eight sections per frame;
/// the capture harness waits for those roots, just as it waits for GLB scenes.
fn attach_sections(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut material: Local<Option<Handle<StandardMaterial>>>,
    sections: Query<(
        Entity,
        &FortificationSegment,
        Option<&shared::economy::GoodsInventory>,
        Option<&FortificationVisual>,
    )>,
) {
    let mut built = 0;
    for (entity, section, inventory, previous) in &sections {
        let supplied =
            !section.complete && inventory.is_some_and(|inventory| inventory.used_bulk() > 0);
        if previous
            .is_some_and(|previous| previous.state == *section && previous.supplied == supplied)
        {
            continue;
        }
        if built == 8 {
            break;
        }
        built += 1;
        let origin = section.midpoint();
        let a = section.start - origin;
        let b = section.end - origin;
        let stone = section.material == FortificationMaterial::Stone;
        let geometry = if !section.complete {
            // Accepted land is visibly surveyed; no unbuilt section looks like
            // an impassable completed wall. Stakes have no ground collider.
            let mut survey = mesh::MasonryMesh::default();
            let color = Vec3::new(0.52, 0.32, 0.12);
            for point in [a, b] {
                survey.beam(
                    point - Vec3::Y * 0.15,
                    point + Vec3::Y * 0.65,
                    0.12,
                    0.12,
                    color,
                );
            }
            survey.finish()
        } else {
            match section.kind {
                FortificationKind::Wall => mesh::wall(
                    a,
                    b,
                    section.material.height(),
                    section.material.thickness(),
                    stone,
                ),
                FortificationKind::Gate => mesh::gate(a, b, section.material),
            }
        };
        let material = material.get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 0.94,
                ..default()
            })
        });
        commands.entity(entity).insert((
            FortificationVisual {
                state: section.clone(),
                supplied,
            },
            Name::new(match (section.complete, section.kind) {
                (false, _) => "Surveyed defense section",
                (true, FortificationKind::Wall) => "Town wall",
                (true, FortificationKind::Gate) => "Open civic gateway",
            }),
            Mesh3d(meshes.add(geometry)),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(origin),
            if section.complete || supplied {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            },
        ));
    }
}

pub(crate) fn visual_matches(
    section: &FortificationSegment,
    visual: Option<&FortificationVisual>,
) -> bool {
    visual.is_some_and(|visual| visual.state == *section)
}
