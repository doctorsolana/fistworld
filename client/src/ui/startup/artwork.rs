//! One set of small reusable surfaces and one scene image for all startup states.

use bevy::{
    prelude::*,
    sprite::{BorderRect, SliceScaleMode, TextureSlicer},
    ui::VisualBox,
};

#[derive(Resource)]
pub(crate) struct StartupArtwork {
    pub(crate) village: Handle<Image>,
    pub(crate) wordmark: Handle<Image>,
    pub(crate) compact_wordmark: Handle<Image>,
    pub(crate) compass_ring: Handle<Image>,
    pub(crate) compass_star: Handle<Image>,
    paper: Handle<Image>,
    frame: Handle<Image>,
    gold: Handle<Image>,
    brass: Handle<Image>,
    wood_button: Handle<Image>,
    input: Handle<Image>,
}

impl FromWorld for StartupArtwork {
    fn from_world(world: &mut World) -> Self {
        let assets = world.resource::<AssetServer>();
        Self {
            village: assets.load("ui/startup/launcher-village.jpg"),
            wordmark: assets.load("ui/startup/wordmark.png"),
            compact_wordmark: assets.load("ui/startup/wordmark-compact.png"),
            compass_ring: assets.load("ui/startup/loading-ring.png"),
            compass_star: assets.load("ui/startup/loading-star.png"),
            paper: assets.load("ui/startup/panel-paper.png"),
            frame: assets.load("ui/creator/frame.png"),
            gold: assets.load("ui/creator/journey.png"),
            brass: assets.load("ui/creator/brass.png"),
            wood_button: assets.load("ui/startup/dark-button.png"),
            input: assets.load("ui/startup/input-field.png"),
        }
    }
}

fn sliced(image: Handle<Image>, border: f32) -> ImageNode {
    ImageNode {
        image,
        image_mode: NodeImageMode::Sliced(TextureSlicer {
            border: BorderRect::all(border),
            ..default()
        }),
        visual_box: VisualBox::BorderBox,
        ..default()
    }
}

impl StartupArtwork {
    pub(crate) fn ready(&self, assets: &AssetServer) -> bool {
        [
            &self.village,
            &self.wordmark,
            &self.compact_wordmark,
            &self.input,
            &self.compass_ring,
            &self.compass_star,
            &self.paper,
            &self.frame,
            &self.gold,
            &self.brass,
            &self.wood_button,
        ]
        .into_iter()
        .all(|image| assets.is_loaded_with_dependencies(image.id()))
    }
    pub(crate) fn paper(&self) -> ImageNode {
        sliced(self.paper.clone(), 38.0)
    }
    pub(crate) fn frame(&self) -> ImageNode {
        let mut image = sliced(self.frame.clone(), 60.0);
        if let NodeImageMode::Sliced(slicer) = &mut image.image_mode {
            slicer.sides_scale_mode = SliceScaleMode::Tile { stretch_value: 1.0 };
        }
        image
    }
    pub(crate) fn gold(&self) -> ImageNode {
        sliced(self.gold.clone(), 26.0)
    }
    pub(crate) fn brass(&self) -> ImageNode {
        ImageNode::new(self.brass.clone())
    }
    pub(crate) fn input(&self) -> ImageNode {
        sliced(self.input.clone(), 18.0)
    }
    pub(crate) fn dark_button(&self) -> ImageNode {
        sliced(self.wood_button.clone(), 24.0)
    }
}
