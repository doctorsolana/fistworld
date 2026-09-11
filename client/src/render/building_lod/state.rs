use super::assets::Library;
use bevy::prelude::*;
use std::sync::Arc;

/// Capture-only override; production buildings always select by projected size.
#[derive(Component)]
pub(crate) struct BuildingLodOverride(pub usize);

#[derive(Component)]
pub(crate) struct BuildingLod {
    pub(super) library: Arc<Library>,
    pub(super) bindings: Vec<(Entity, usize)>,
    pub(crate) level: usize,
    pub(crate) ready: bool,
    pub(super) failed: bool,
    pub(super) needs_refresh: bool,
    pub(super) visibility_before_hide: Option<Visibility>,
}

impl BuildingLod {
    pub(crate) fn triangles(&self) -> [usize; 2] {
        [
            self.library.triangles[0],
            if self.level == super::HIDDEN {
                0
            } else {
                self.library.triangles[self.level]
            },
        ]
    }
}
