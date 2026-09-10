//! One session policy owns both reads and writes of the player's settings.
//! Capture fixtures mutate graphics directly, so stripping environment overrides
//! alone cannot keep those test values out of the player's saved preferences.

use super::{GraphicsSettings, PendingDisplayChange, SETTINGS_FILE};
use bevy::prelude::*;
use std::path::PathBuf;

#[derive(Resource)]
pub struct GraphicsSettingsStore {
    path: PathBuf,
    enabled: bool,
}

impl Default for GraphicsSettingsStore {
    fn default() -> Self {
        Self {
            path: SETTINGS_FILE.into(),
            enabled: std::env::var_os("FISTFORCE_NO_SETTINGS_FILE").is_none(),
        }
    }
}

impl GraphicsSettingsStore {
    fn read(&self) -> Option<GraphicsSettings> {
        if !self.enabled {
            return None;
        }
        let text = std::fs::read_to_string(&self.path).ok()?;
        match ron::from_str(&text) {
            Ok(settings) => Some(settings),
            Err(err) => {
                warn!("Ignoring malformed {}: {err}", self.path.display());
                None
            }
        }
    }

    pub fn load(&self) -> GraphicsSettings {
        let mut settings = self
            .read()
            .unwrap_or_else(GraphicsSettings::shipped_defaults);
        // Test overrides still apply when file access is disabled.
        settings.apply_env_overrides();
        settings
    }

    fn save(&self, settings: &GraphicsSettings) {
        if !self.enabled {
            return;
        }
        let baseline = self
            .read()
            .unwrap_or_else(GraphicsSettings::shipped_defaults);
        let mut to_save = settings.clone();
        to_save.revert_env_forced(&baseline);
        let serialized =
            match ron::ser::to_string_pretty(&to_save, ron::ser::PrettyConfig::default()) {
                Ok(text) => text,
                Err(err) => {
                    warn!("Could not serialize graphics settings: {err}");
                    return;
                }
            };
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&self.path, serialized) {
            Ok(()) => info!("Saved graphics settings to {}", self.path.display()),
            Err(err) => warn!("Could not write {}: {err}", self.path.display()),
        }
    }
}

/// Debounce real player changes; an unconfirmed display mode never reaches disk.
pub fn save_graphics_settings(
    settings: Res<GraphicsSettings>,
    store: Res<GraphicsSettingsStore>,
    time: Res<Time>,
    pending_display: Option<Res<PendingDisplayChange>>,
    mut deadline: Local<Option<f32>>,
) {
    if !store.enabled {
        *deadline = None;
        return;
    }
    if settings.is_changed() && !settings.is_added() {
        *deadline = Some(time.elapsed_secs() + 1.0);
    }
    let Some(due) = *deadline else { return };
    if pending_display.is_some() || time.elapsed_secs() < due {
        return;
    }
    *deadline = None;
    store.save(&settings);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::Duration,
    };

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "fistworld-settings-{}-{}",
                std::process::id(),
                NEXT_TEST.fetch_add(1, Ordering::Relaxed),
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn app(path: PathBuf, enabled: bool) -> App {
        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .insert_resource(GraphicsSettings::shipped_defaults())
            .insert_resource(GraphicsSettingsStore { path, enabled })
            .add_systems(Update, save_graphics_settings);
        app.update();
        app
    }

    fn fixture_changes(app: &mut App) {
        let mut settings = app.world_mut().resource_mut::<GraphicsSettings>();
        settings.props_enabled = false;
        settings.fullscreen_enabled = false;
        settings.render_scale = 1.0;
        app.update();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs(2));
        app.update();
    }

    #[test]
    fn isolated_capture_does_not_read_overwrite_or_create_player_settings() {
        let dir = TestDirectory::new();
        let path = dir.0.join("settings.ron");
        let saved = b"(props_enabled: true, render_scale: 0.6) // player preference\n";
        std::fs::write(&path, saved).unwrap();
        let mut app = app(path.clone(), false);
        assert!(app
            .world()
            .resource::<GraphicsSettingsStore>()
            .read()
            .is_none());
        fixture_changes(&mut app);
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        std::fs::remove_file(&path).unwrap();
        fixture_changes(&mut app);
        assert!(
            !path.exists(),
            "isolated test runs must not create a settings file"
        );
    }

    #[test]
    fn normal_settings_save_waits_for_display_confirmation_and_roundtrips() {
        let dir = TestDirectory::new();
        let path = dir.0.join("settings.ron");
        let mut app = app(path.clone(), true);
        let pending = PendingDisplayChange::new(app.world().resource::<GraphicsSettings>());
        app.insert_resource(pending);
        fixture_changes(&mut app);
        assert!(!path.exists(), "a candidate mode must not be persisted");
        app.world_mut().remove_resource::<PendingDisplayChange>();
        app.update();
        let saved = app
            .world()
            .resource::<GraphicsSettingsStore>()
            .read()
            .unwrap();
        assert!(!saved.props_enabled);
        assert!(!saved.fullscreen_enabled);
        assert_eq!(saved.render_scale, 1.0);
    }

    #[test]
    fn partial_saved_settings_keep_shipped_defaults_before_session_overrides() {
        const CHILD: &str = "FISTWORLD_TEST_PARTIAL_SETTINGS_CHILD";
        if std::env::var_os(CHILD).is_none() {
            // Process-local overrides must not race other tests using Default.
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .arg("partial_saved_settings_keep_shipped_defaults_before_session_overrides")
                .arg("--nocapture")
                .env(CHILD, "1")
                .env("FISTFORCE_PROPS", "0")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "isolated settings regression failed:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
            return;
        }

        assert!(!GraphicsSettings::default().props_enabled);
        let dir = TestDirectory::new();
        let store = GraphicsSettingsStore {
            path: dir.0.join("partial-settings.ron"),
            enabled: true,
        };
        std::fs::write(&store.path, "(render_scale: 0.6)").unwrap();
        let baseline = store.read().unwrap();
        assert!(
            baseline.props_enabled,
            "missing fields use shipped defaults"
        );
        assert_eq!(baseline.render_scale, 0.6);

        let mut session = store.load();
        assert!(
            !session.props_enabled,
            "session override still takes effect"
        );
        session.grade_exposure = 0.7;
        store.save(&session);
        let saved = store.read().unwrap();
        assert!(saved.props_enabled, "the override must not reach the file");
        assert_eq!(saved.grade_exposure, 0.7);
    }
}
