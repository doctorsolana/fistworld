//! Hero rendering + control: the client half of the embodied character.
//!
//! The server owns hero position/rotation (see server/src/player/hero.rs);
//! this module attaches the canonical humanoid glb to replicated Hero entities,
//! dresses them from their replicated [`HeroOutfit`], drives the walk
//! animation from observed velocity, smooths the streamed transform, and
//! turns clicks into spawn/move commands.
//!
//! This facade owns plugin scheduling. Appearance, snapshot motion, animation,
//! attachments and carts each own their local state and systems.

pub mod control;
pub mod footprints;

mod animation;
mod appearance;
mod attachments;
mod carts;
mod motion;

use crate::states::GameState;
use animation::{drive_hero_locomotion, setup_hero_animation};
use appearance::{
    apply_hero_skin, attach_hero_visuals, dress_heroes, matte_character_materials,
    sync_indoor_visibility,
};
use attachments::{
    sync_carried_load_visuals, sync_tool_visuals, tag_carry_attachments, tag_character_heads,
    tag_tool_attachments, CarriedLoadAssets, ToolAssets,
};
use bevy::prelude::*;
use carts::{
    drive_porter_cart_motion, recover_stale_porter_cart_animation, setup_porter_cart_animation,
    sync_porter_cart_load_visuals, sync_porter_cart_visuals, tag_porter_cart_load_attachments,
    PorterCartAssets,
};
use shared::character::CharacterManifest;
use shared::components::HeroOutfit;

pub use animation::HeroGraph;
pub(crate) use appearance::{spawn_character_scene_child, HeroDressed, HeroFullRig};
pub use appearance::{HeroAssets, HeroManifest, HeroPreviewRig};
pub(crate) use attachments::CharacterHead;
pub(crate) use motion::sync_hero_transforms;
pub use motion::HeroVisual;

pub struct HeroPlugin;

impl Plugin for HeroPlugin {
    fn build(&self, app: &mut App) {
        // The manifest ships beside the glb; a missing or malformed one means
        // a broken asset build, so fail loudly at startup rather than
        // rendering bald, naked heroes.
        let manifest = CharacterManifest::load()
            .unwrap_or_else(|e| panic!("character manifest could not be loaded: {e}"));
        info!(
            "Character manifest: {} slots, {} skin tones, {} body + {} face clips",
            manifest.slots.len(),
            manifest.skin.tones.len(),
            manifest.body_clips.len(),
            manifest.face_clips.len()
        );
        // The creator starts on the look the art build declares as default.
        app.insert_resource(control::SelectedOutfit(HeroOutfit::from_manifest(
            &manifest,
        )));
        app.insert_resource(HeroManifest(manifest));
        app.init_resource::<HeroAssets>();
        app.init_resource::<CarriedLoadAssets>();
        app.init_resource::<PorterCartAssets>();
        app.init_resource::<ToolAssets>();
        app.init_resource::<control::WorldPlacementMode>();
        app.init_resource::<footprints::FootprintPool>();
        app.init_resource::<footprints::StrideTrackers>();
        app.add_systems(Startup, footprints::setup_footprint_assets);
        app.add_systems(
            Update,
            (footprints::stamp_footprints, footprints::fade_footprints)
                .run_if(in_state(crate::states::GameState::Playing)),
        );
        app.add_systems(
            Update,
            (
                (
                    attach_hero_visuals,
                    dress_heroes,
                    matte_character_materials,
                    apply_hero_skin,
                    setup_hero_animation,
                    sync_hero_transforms,
                ),
                (
                    tag_carry_attachments,
                    tag_tool_attachments,
                    tag_character_heads,
                    sync_indoor_visibility,
                    sync_porter_cart_visuals,
                    sync_carried_load_visuals,
                    sync_tool_visuals,
                    (
                        tag_porter_cart_load_attachments,
                        sync_porter_cart_load_visuals,
                    )
                        .chain(),
                    (
                        recover_stale_porter_cart_animation,
                        setup_porter_cart_animation,
                        drive_hero_locomotion,
                        drive_porter_cart_motion,
                    )
                        .chain(),
                ),
                (
                    control::handle_world_clicks,
                    control::auto_spawn_hero,
                    control::auto_set_time_warp_after,
                    control::auto_set_time_of_day,
                ),
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
    }
}

#[cfg(test)]
mod tests;
