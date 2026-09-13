//! Local audio preferences, independent of graphics and simulation settings.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Resource, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AudioSettings {
    pub music_enabled: bool,
    pub effects_enabled: bool,
    pub master_volume: f32,
    pub music_volume: f32,
    pub effects_volume: f32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            music_enabled: true,
            effects_enabled: true,
            master_volume: 1.0,
            music_volume: 0.5,
            effects_volume: 1.0,
        }
    }
}

impl AudioSettings {
    fn level(value: f32) -> f32 {
        if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

    pub(crate) fn music_gain(&self) -> f32 {
        if self.music_enabled {
            Self::level(self.master_volume) * Self::level(self.music_volume)
        } else {
            0.0
        }
    }

    pub(crate) fn effects_gain(&self) -> f32 {
        if self.effects_enabled {
            Self::level(self.master_volume) * Self::level(self.effects_volume)
        } else {
            0.0
        }
    }

    fn sanitize(&mut self) {
        self.master_volume = Self::level(self.master_volume);
        self.music_volume = Self::level(self.music_volume);
        self.effects_volume = Self::level(self.effects_volume);
    }
}

#[derive(Resource)]
pub(super) struct AudioSettingsStore {
    path: PathBuf,
    enabled: bool,
}

impl Default for AudioSettingsStore {
    fn default() -> Self {
        Self {
            path: "client_data/audio.ron".into(),
            enabled: std::env::var_os("FISTFORCE_NO_SETTINGS_FILE").is_none(),
        }
    }
}

impl AudioSettingsStore {
    pub(super) fn load(&self) -> AudioSettings {
        if !self.enabled {
            return AudioSettings::default();
        }
        match std::fs::read_to_string(&self.path) {
            Ok(text) => ron::from_str(&text)
                .map(|mut settings: AudioSettings| {
                    settings.sanitize();
                    settings
                })
                .unwrap_or_else(|error| {
                    warn!("Ignoring malformed {}: {error}", self.path.display());
                    AudioSettings::default()
                }),
            Err(_) => AudioSettings::default(),
        }
    }

    fn save(&self, settings: &AudioSettings) -> std::io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = ron::ser::to_string_pretty(settings, ron::ser::PrettyConfig::default())
            .map_err(std::io::Error::other)?;
        let temporary = self.path.with_extension("ron.tmp");
        std::fs::write(&temporary, text)?;
        std::fs::rename(temporary, &self.path)
    }
}

#[derive(Default)]
pub(super) struct PendingSave {
    settings: Option<AudioSettings>,
    edited_at: f64,
}

pub(super) fn save_audio_settings(
    settings: Res<AudioSettings>,
    store: Res<AudioSettingsStore>,
    time: Res<Time<Real>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut exit: MessageReader<bevy::app::AppExit>,
    mut pending: Local<PendingSave>,
) {
    // Apply the mix immediately, but commit a drag once it stops. A normal quit
    // flushes the final value even when it lands inside the debounce interval.
    let quitting = exit.read().next().is_some();
    if !store.enabled {
        return;
    }
    if settings.is_changed() && !settings.is_added() {
        pending.settings = Some(settings.clone());
        pending.edited_at = time.elapsed_secs_f64();
    }
    if quitting
        || (time.elapsed_secs_f64() - pending.edited_at >= 0.25
            && !mouse.pressed(MouseButton::Left))
    {
        if let Some(settings) = pending.settings.take() {
            if let Err(error) = store.save(&settings) {
                warn!("Could not save audio preferences: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn music_setting_roundtrips_and_capture_cannot_read_or_overwrite_it() {
        let directory =
            std::env::temp_dir().join(format!("fistworld-audio-settings-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("audio.ron");
        let mut store = AudioSettingsStore {
            path: path.clone(),
            enabled: true,
        };
        store
            .save(&AudioSettings {
                music_enabled: false,
                effects_enabled: false,
                music_volume: 0.3,
                ..Default::default()
            })
            .unwrap();
        assert!(!store.load().music_enabled);
        assert!(!store.load().effects_enabled);
        assert_eq!(store.load().music_volume, 0.3);
        let saved = std::fs::read(&path).unwrap();
        store.enabled = false;
        assert!(store.load().music_enabled);
        assert_eq!(store.load().music_volume, 0.5);
        store.save(&AudioSettings::default()).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        std::fs::remove_file(&path).unwrap();
        store.save(&AudioSettings::default()).unwrap();
        assert!(!path.exists());
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn missing_fields_keep_music_on() {
        assert!(ron::from_str::<AudioSettings>("()").unwrap().music_enabled);
        assert_eq!(
            ron::from_str::<AudioSettings>("()").unwrap().music_volume,
            0.5
        );
        let old: AudioSettings = ron::from_str("(music_enabled: false)").unwrap();
        assert!(!old.music_enabled);
        assert!(old.effects_enabled);
        assert_eq!(old.music_volume, 0.5);
        let saved: AudioSettings = ron::from_str("(music_volume: 1.0)").unwrap();
        assert_eq!(
            saved.music_volume, 1.0,
            "an existing chosen level is preserved"
        );
    }
}

#[cfg(test)]
mod levels_tests {
    use super::*;
    #[test]
    fn master_and_category_mix_safely_and_sanitize_invalid_saved_values() {
        let mut settings = AudioSettings {
            master_volume: 0.5,
            music_volume: 0.2,
            effects_volume: 0.8,
            ..Default::default()
        };
        assert_eq!(settings.music_gain(), 0.1);
        assert_eq!(settings.effects_gain(), 0.4);
        settings.effects_enabled = false;
        assert_eq!(settings.effects_gain(), 0.0);
        settings.master_volume = f32::NAN;
        settings.music_volume = 20.0;
        settings.effects_volume = -1.0;
        settings.sanitize();
        assert_eq!(
            (
                settings.master_volume,
                settings.music_volume,
                settings.effects_volume
            ),
            (1.0, 1.0, 0.0)
        );
    }
}
