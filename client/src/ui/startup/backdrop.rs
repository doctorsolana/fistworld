//! One cover-fitted illustration. A small fixed filter softens it behind the modals.

use super::StartupArtwork;
use crate::states::GameState;
use bevy::{prelude::*, render::render_resource::AsBindGroup, shader::ShaderRef};

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(crate) struct StartupBackdropMaterial {
    #[uniform(0)]
    finish: Vec4,
    #[texture(1)]
    #[sampler(2)]
    image: Handle<Image>,
}

impl UiMaterial for StartupBackdropMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/ui/startup_backdrop.wgsl".into()
    }
}

impl StartupBackdropMaterial {
    /// Samples the existing pre-UI scene target; no second world render or copy.
    pub(crate) fn live_scene(image: Handle<Image>) -> Self {
        Self {
            finish: Vec4::new(0.30, 1.6, 0.0, 0.0),
            image,
        }
    }
}

#[derive(Component)]
struct StartupBackdrop;

pub(super) fn install(app: &mut App) {
    app.add_plugins(UiMaterialPlugin::<StartupBackdropMaterial>::default())
        .add_systems(Update, sync_backdrop);
}

fn sync_backdrop(
    mut commands: Commands,
    state: Res<State<GameState>>,
    art: Res<StartupArtwork>,
    mut materials: ResMut<Assets<StartupBackdropMaterial>>,
    roots: Query<(Entity, &MaterialNode<StartupBackdropMaterial>), With<StartupBackdrop>>,
) {
    if *state.get() == GameState::Playing {
        for (entity, material) in &roots {
            // Drop the state-specific GPU material, retaining only the shared art handles.
            materials.remove(material.0.id());
            commands.entity(entity).despawn();
        }
        return;
    }
    let finish = if *state.get() == GameState::MainMenu {
        Vec4::new(0.0, 0.0, 0.55, 0.0)
    } else {
        Vec4::new(0.24, 1.45, 0.35, 0.0)
    };
    if roots.is_empty() {
        commands.spawn((
            StartupBackdrop,
            Name::new("startup-backdrop"),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            MaterialNode(materials.add(StartupBackdropMaterial {
                finish,
                image: art.village.clone(),
            })),
            GlobalZIndex(80),
            Pickable::IGNORE,
        ));
    } else if state.is_changed() {
        for (_, node) in &roots {
            if let Some(mut material) = materials.get_mut(&node.0) {
                material.finish = finish;
            }
        }
    }
}
