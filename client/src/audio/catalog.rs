//! Only approved runtime files belong here. Retaining this tiny bank prevents
//! first-click asset churn; an unavailable cue never blocks the rest of the bank.

use bevy::{asset::LoadState, prelude::*};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(usize)]
pub(crate) enum SfxCue {
    UiClick,
    UiConfirm,
    UiReject,
    BookOpen,
    BookClose,
    PageTurn,
    CartRoll,
}

impl SfxCue {
    pub(crate) const ALL: [Self; 7] = [
        Self::UiClick,
        Self::UiConfirm,
        Self::UiReject,
        Self::BookOpen,
        Self::BookClose,
        Self::PageTurn,
        Self::CartRoll,
    ];

    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::UiClick => "audio/sfx/ui/ui_click.wav",
            Self::UiConfirm => "audio/sfx/ui/ui_confirm.wav",
            Self::UiReject => "audio/sfx/ui/ui_reject.wav",
            Self::BookOpen => "audio/sfx/ui/book_open.wav",
            Self::BookClose => "audio/sfx/ui/book_close.wav",
            Self::PageTurn => "audio/sfx/ui/page_turn.wav",
            Self::CartRoll => "audio/sfx/vehicles/cart_roll.ogg",
        }
    }

    pub(crate) fn gain(self) -> f32 {
        match self {
            Self::UiClick | Self::UiConfirm => 0.26,
            Self::UiReject => 0.22,
            // This source is deliberately very soft, even after preparation.
            Self::BookOpen => 0.70,
            Self::BookClose => 0.25,
            Self::PageTurn => 0.28,
            Self::CartRoll => 0.14,
        }
    }

    pub(crate) fn priority(self) -> u8 {
        match self {
            Self::BookOpen | Self::BookClose => 4,
            Self::UiConfirm | Self::UiReject => 3,
            Self::PageTurn => 2,
            Self::UiClick => 1,
            Self::CartRoll => 0,
        }
    }
}

#[derive(Resource)]
pub(crate) struct SfxAssets {
    handles: [Handle<AudioSource>; 7],
    reported_failed: [bool; 7],
}

impl FromWorld for SfxAssets {
    fn from_world(world: &mut World) -> Self {
        let assets = world.resource::<AssetServer>();
        Self {
            handles: SfxCue::ALL.map(|cue| assets.load(cue.path())),
            reported_failed: [false; 7],
        }
    }
}

impl SfxAssets {
    pub(crate) fn ready_handle(
        &self,
        cue: SfxCue,
        assets: &AssetServer,
    ) -> Option<Handle<AudioSource>> {
        let handle = &self.handles[cue as usize];
        matches!(assets.get_load_state(handle), Some(LoadState::Loaded)).then(|| handle.clone())
    }

    pub(super) fn report_failures(&mut self, assets: &AssetServer) {
        for cue in SfxCue::ALL {
            let index = cue as usize;
            if !self.reported_failed[index]
                && matches!(
                    assets.get_load_state(&self.handles[index]),
                    Some(LoadState::Failed(_))
                )
            {
                self.reported_failed[index] = true;
                warn!("Sound effect unavailable: {}", cue.path());
            }
        }
    }
}
