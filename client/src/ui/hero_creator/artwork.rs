//! Small text-free creator surfaces. All choices and lettering remain native UI.
use bevy::{
    prelude::*,
    sprite::{BorderRect, SliceScaleMode, TextureSlicer},
    ui::VisualBox,
};

#[derive(Resource)]
pub(crate) struct CreatorArtwork {
    paper: Handle<Image>,
    brass: Handle<Image>,
    journey: Handle<Image>,
    frame: Handle<Image>,
}

impl FromWorld for CreatorArtwork {
    fn from_world(world: &mut World) -> Self {
        let assets = world.resource::<AssetServer>();
        Self {
            paper: assets.load("ui/creator/paper.png"),
            brass: assets.load("ui/creator/brass.png"),
            journey: assets.load("ui/creator/journey.png"),
            frame: assets.load("ui/creator/frame.png"),
        }
    }
}

fn surface(image: Handle<Image>, border: f32) -> ImageNode {
    ImageNode {
        image,
        visual_box: VisualBox::BorderBox,
        image_mode: NodeImageMode::Sliced(TextureSlicer {
            border: BorderRect::all(border),
            ..default()
        }),
        ..default()
    }
}

impl CreatorArtwork {
    pub(crate) fn ready(&self, assets: &AssetServer) -> bool {
        [&self.paper, &self.brass, &self.journey, &self.frame]
            .into_iter()
            .all(|handle| assets.is_loaded_with_dependencies(handle.id()))
    }

    pub(super) fn paper(&self) -> ImageNode {
        surface(self.paper.clone(), 38.0)
    }
    pub(super) fn brass(&self) -> ImageNode {
        ImageNode {
            image: self.brass.clone(),
            ..default()
        }
    }
    pub(super) fn journey(&self) -> ImageNode {
        surface(self.journey.clone(), 26.0)
    }
    pub(super) fn frame(&self) -> ImageNode {
        let mut frame = surface(self.frame.clone(), 60.0);
        if let NodeImageMode::Sliced(slicer) = &mut frame.image_mode {
            // Keep tiny pits/chips at their authored scale on long rails.
            slicer.sides_scale_mode = SliceScaleMode::Tile { stretch_value: 1.0 };
        }
        frame
    }
    pub(super) fn inset(&self) -> ImageNode {
        surface(self.paper.clone(), 18.0)
    }
}
