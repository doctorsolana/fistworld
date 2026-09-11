//! One shared material per illustration/finish. Worn edges remain transparent
//! over the actual page; no screen-sized masks or per-widget image copies.

use super::artwork::LedgerIllustration;
use bevy::{prelude::*, render::render_resource::AsBindGroup, shader::ShaderRef};
use std::collections::HashMap;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum IllustrationFinish {
    Plain,
    Vignette,
    Round,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(crate) struct IllustrationMaterial {
    #[uniform(0)]
    finish: Vec4,
    #[texture(1)]
    #[sampler(2)]
    image: Handle<Image>,
}

impl UiMaterial for IllustrationMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/ui/ledger_illustration.wgsl".into()
    }
}

#[derive(Resource, Default)]
pub(crate) struct IllustrationMaterials {
    handles: HashMap<(LedgerIllustration, IllustrationFinish), Handle<IllustrationMaterial>>,
}

impl IllustrationMaterial {
    pub(crate) fn ready_for(&self, kind: LedgerIllustration, assets: &AssetServer) -> bool {
        self.image == assets.load::<Image>(kind.path()) && self.ready(assets)
    }

    pub(crate) fn ready(&self, assets: &AssetServer) -> bool {
        assets.is_loaded_with_dependencies(self.image.id())
    }
}

pub(super) fn bind_illustrations(
    assets: Res<AssetServer>,
    mut materials: ResMut<Assets<IllustrationMaterial>>,
    mut cache: ResMut<IllustrationMaterials>,
    mut illustrations: Query<
        (
            &LedgerIllustration,
            &IllustrationFinish,
            &mut MaterialNode<IllustrationMaterial>,
        ),
        Or<(Changed<LedgerIllustration>, Changed<IllustrationFinish>)>,
    >,
) {
    for (&kind, &finish, mut node) in &mut illustrations {
        let handle = cache.handles.entry((kind, finish)).or_insert_with(|| {
            materials.add(IllustrationMaterial {
                finish: Vec4::new(
                    match finish {
                        IllustrationFinish::Plain => 0.0,
                        IllustrationFinish::Vignette => 1.0,
                        IllustrationFinish::Round => 2.0,
                    },
                    0.0,
                    0.0,
                    0.0,
                ),
                image: assets.load(kind.path()),
            })
        });
        if node.0 != *handle {
            node.0 = handle.clone();
        }
    }
}
