//! Reusable capture-scenario and artifact support.
//!
//! This module deliberately contains no world staging. [`crate::capture`] still
//! boots the real client and creates its visual fixtures; this layer describes
//! what to photograph, writes screenshots from Bevy's completion observer, and
//! turns every PNG into a machine-verifiable artifact.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use image::{DynamicImage, RgbImage};
use serde::{Deserialize, Serialize};

use crate::capture::Shot;

pub const CAPTURE_SCENARIO_VERSION: u32 = 1;
pub const CAPTURE_METADATA_VERSION: u32 = 1;

/// The surface copied by the renderer.
///
/// `Window` includes native-resolution UI. `Scene` reads the game's stable
/// offscreen 3D target directly, making it independent of fullscreen mode and
/// display scaling (but intentionally excluding UI).
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptureTarget {
    #[default]
    Window,
    Scene,
}

/// Conditions that must remain true before a shot is requested.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CaptureReadiness {
    /// Minimum rendered frames after the camera reaches this shot.
    pub minimum_frames: u32,
    /// Fail rather than silently hang if readiness never arrives.
    pub maximum_frames: u32,
    /// Minimum number of detailed terrain chunks around the camera.
    pub minimum_loaded_chunks: usize,
    /// Require the loaded-chunk count to stop changing for this many frames.
    pub stable_loaded_chunk_frames: u32,
}

impl Default for CaptureReadiness {
    fn default() -> Self {
        Self {
            minimum_frames: 60,
            maximum_frames: 1_200,
            minimum_loaded_chunks: 1,
            stable_loaded_chunk_frames: 12,
        }
    }
}

/// A semantic invariant evaluated immediately before a screenshot.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureAssertion {
    LoadedChunksAtLeast { count: usize },
    EntitiesAtLeast { count: usize },
    VillagersAtLeast { count: usize },
    SettlementsAtLeast { count: usize },
    PlanningRoutesAtMost { count: usize },
    BlockedRoutesAtMost { count: usize },
}

/// Optional pixel-baseline policy.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CaptureComparisonConfig {
    pub baseline_dir: PathBuf,
    /// Replace/create baselines instead of comparing against them.
    pub update_baselines: bool,
    /// A pixel counts as changed if any RGB channel differs by this fraction.
    pub pixel_threshold: f32,
    /// Maximum average absolute RGB error, normalized to `0..=1`.
    pub maximum_mean_error: f32,
    /// Maximum fraction of changed pixels.
    pub maximum_changed_fraction: f32,
}

impl Default for CaptureComparisonConfig {
    fn default() -> Self {
        Self {
            baseline_dir: PathBuf::from("capture/baselines"),
            update_baselines: false,
            pixel_threshold: 0.04,
            maximum_mean_error: 0.01,
            maximum_changed_fraction: 0.02,
        }
    }
}

/// Declarative, checked-in input for the offline capture binary.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CaptureScenario {
    pub version: u32,
    pub name: String,
    pub map: String,
    pub output_dir: PathBuf,
    pub resolution: [u32; 2],
    pub fixed_delta_seconds: f64,
    pub target: CaptureTarget,
    pub show_window: bool,
    pub warmup_frames: u32,
    pub readiness: CaptureReadiness,
    pub continuous: bool,
    pub probe_every: u32,
    pub environment: BTreeMap<String, String>,
    pub comparison: Option<CaptureComparisonConfig>,
    pub recording: Option<CaptureRecordingConfig>,
    pub diagnostics: CaptureDiagnosticsConfig,
    pub shots: Vec<CaptureScenarioShot>,
}

impl Default for CaptureScenario {
    fn default() -> Self {
        Self {
            version: CAPTURE_SCENARIO_VERSION,
            name: "capture".to_owned(),
            map: "big_world".to_owned(),
            output_dir: PathBuf::from("/tmp/fistworld-captures"),
            resolution: [1_600, 900],
            fixed_delta_seconds: 1.0 / 60.0,
            target: CaptureTarget::Window,
            show_window: true,
            warmup_frames: 240,
            readiness: CaptureReadiness::default(),
            continuous: false,
            probe_every: 4,
            environment: BTreeMap::new(),
            comparison: None,
            recording: None,
            diagnostics: CaptureDiagnosticsConfig::default(),
            shots: vec![CaptureScenarioShot::default()],
        }
    }
}

impl CaptureScenario {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read scenario {}: {error}", path.display()))?;
        let scenario: Self = ron::from_str(&text)
            .map_err(|error| format!("cannot parse scenario {}: {error}", path.display()))?;
        scenario.validate()?;
        Ok(scenario)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != CAPTURE_SCENARIO_VERSION {
            return Err(format!(
                "unsupported capture scenario version {} (expected {})",
                self.version, CAPTURE_SCENARIO_VERSION
            ));
        }
        if self.name.trim().is_empty() {
            return Err("capture scenario name cannot be empty".to_owned());
        }
        if self.shots.is_empty() {
            return Err("capture scenario needs at least one shot".to_owned());
        }
        if self.resolution[0] == 0 || self.resolution[1] == 0 {
            return Err("capture resolution must be non-zero".to_owned());
        }
        if !self.fixed_delta_seconds.is_finite() || self.fixed_delta_seconds <= 0.0 {
            return Err("fixed_delta_seconds must be finite and positive".to_owned());
        }
        validate_readiness(&self.readiness)?;
        if self.continuous && self.probe_every == 0 {
            return Err("continuous scenarios require probe_every >= 1".to_owned());
        }
        for shot in &self.shots {
            if shot.name.trim().is_empty() {
                return Err("capture shot name cannot be empty".to_owned());
            }
            if shot.focus.iter().any(|value| !value.is_finite())
                || !shot.yaw.is_finite()
                || !shot.zoom.is_finite()
                || shot.zoom <= 0.0
                || !shot.time_of_day.is_finite()
                || !(0.0..=1.0).contains(&shot.time_of_day)
                || shot.pitch.is_some_and(|pitch| !pitch.is_finite())
                || !shot.eye.is_finite()
            {
                return Err(format!(
                    "capture shot '{}' contains an invalid camera value",
                    shot.name
                ));
            }
            if let Some(readiness) = &shot.readiness {
                validate_readiness(readiness)?;
            }
        }
        if let Some(comparison) = &self.comparison {
            for (label, value) in [
                ("pixel_threshold", comparison.pixel_threshold),
                ("maximum_mean_error", comparison.maximum_mean_error),
                (
                    "maximum_changed_fraction",
                    comparison.maximum_changed_fraction,
                ),
            ] {
                if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                    return Err(format!("{label} must be in 0..=1"));
                }
            }
        }
        if let Some(recording) = &self.recording {
            if recording.enabled && recording.frame_rate == 0 {
                return Err("recording frame_rate must be non-zero".to_owned());
            }
            if !recording.flush_seconds.is_finite() || recording.flush_seconds < 0.0 {
                return Err("recording flush_seconds must be finite and non-negative".to_owned());
            }
        }
        Ok(())
    }

    /// Apply environment-driven fixture settings before any Bevy plugin reads
    /// them. Scenario files are trusted developer inputs, like `run.sh`.
    pub fn apply_environment(&self) {
        std::env::set_var("CITYSIM_MAP_ID", &self.map);
        if self.diagnostics.render_timings {
            std::env::set_var("FISTFORCE_RENDER_DIAG", "1");
            std::env::set_var("FISTFORCE_LOG_DIAGNOSTICS", "1");
        }
        for (key, value) in &self.environment {
            std::env::set_var(key, value);
        }
    }
}

/// Developer-only layers that can be embedded in a capture run.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CaptureDiagnosticsConfig {
    /// Draw the F3 performance/world-information overlay in window captures.
    pub performance_overlay: bool,
    /// Draw F4 plot, collider and planning gizmos.
    pub gizmos: bool,
    /// Enable Bevy render timing diagnostics in logs and metadata-adjacent output.
    pub render_timings: bool,
}

fn validate_readiness(readiness: &CaptureReadiness) -> Result<(), String> {
    if readiness.maximum_frames < readiness.minimum_frames {
        return Err("readiness maximum_frames must be >= minimum_frames".to_owned());
    }
    if readiness.maximum_frames == 0 {
        return Err("readiness maximum_frames must be non-zero".to_owned());
    }
    Ok(())
}

/// Optional deterministic recording of the scenario's post-warmup sequence.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CaptureRecordingConfig {
    pub enabled: bool,
    pub frame_rate: u32,
    /// Keep the app alive for this real-time interval after requesting stop so
    /// Bevy's x264 worker can flush the raw H.264 stream.
    pub flush_seconds: f32,
}

impl Default for CaptureRecordingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            frame_rate: 30,
            flush_seconds: 2.0,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CaptureScenarioShot {
    pub name: String,
    pub focus: [f32; 3],
    pub yaw: f32,
    pub zoom: f32,
    pub time_of_day: f32,
    pub pitch: Option<f32>,
    pub eye: f32,
    pub readiness: Option<CaptureReadiness>,
    pub assertions: Vec<CaptureAssertion>,
}

impl Default for CaptureScenarioShot {
    fn default() -> Self {
        Self {
            name: "shot".to_owned(),
            focus: [0.0; 3],
            yaw: -0.45,
            zoom: 220.0,
            time_of_day: 0.5,
            pitch: None,
            eye: 1.7,
            readiness: None,
            assertions: Vec::new(),
        }
    }
}

impl CaptureScenarioShot {
    pub fn into_runtime(self, default_readiness: &CaptureReadiness) -> Shot {
        Shot {
            name: self.name,
            focus: Vec3::from_array(self.focus),
            yaw: self.yaw,
            zoom: self.zoom,
            time_of_day: self.time_of_day,
            pitch: self.pitch,
            eye: self.eye,
            readiness: self.readiness.unwrap_or_else(|| default_readiness.clone()),
            assertions: self.assertions,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CaptureWorldSnapshot {
    pub frame: u32,
    pub entity_count: usize,
    pub loaded_chunks: usize,
    pub villagers: usize,
    pub settlements: usize,
    pub planning_routes: usize,
    pub blocked_routes: usize,
    pub world_day: Option<u32>,
    pub normalized_time: Option<f32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CaptureAssertionResult {
    pub assertion: CaptureAssertion,
    pub passed: bool,
    pub observed: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct CaptureCameraMetadata {
    pub focus: [f32; 3],
    pub yaw: f32,
    pub zoom: f32,
    pub time_of_day: f32,
    pub pitch: Option<f32>,
    pub eye: f32,
    /// Actual 3D camera pose for connected captures, including cinematics
    /// that temporarily override the commander controller's orbit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<[f32; 4]>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CaptureMetadata {
    pub schema_version: u32,
    pub scenario: String,
    pub shot: String,
    pub map: String,
    pub git_commit: String,
    pub target: CaptureTarget,
    pub fixed_delta_seconds: f64,
    pub output: String,
    pub width: u32,
    pub height: u32,
    pub readiness_frames: u32,
    pub camera: CaptureCameraMetadata,
    pub world: CaptureWorldSnapshot,
    pub assertions: Vec<CaptureAssertionResult>,
    pub comparison: Option<CaptureComparisonResult>,
    pub comparison_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CaptureComparisonResult {
    pub baseline: String,
    pub diff: Option<String>,
    pub baseline_updated: bool,
    pub dimensions_match: bool,
    pub mean_error: f32,
    pub changed_fraction: f32,
    pub passed: bool,
}

#[derive(Clone)]
pub struct CaptureWriteRequest {
    pub path: PathBuf,
    pub metadata: CaptureMetadata,
    pub comparison: Option<CaptureComparisonConfig>,
}

#[derive(Clone, Debug)]
pub struct CaptureCompletion {
    pub ticket: u64,
    pub path: PathBuf,
    pub comparison_failed: bool,
    pub error: Option<String>,
}

/// Completion mailbox written by [`ScreenshotCaptured`] observers.
#[derive(Resource, Default)]
pub struct CaptureCompletions {
    next_ticket: u64,
    completed: VecDeque<CaptureCompletion>,
}

impl CaptureCompletions {
    pub fn allocate(&mut self) -> u64 {
        self.next_ticket = self.next_ticket.wrapping_add(1).max(1);
        self.next_ticket
    }

    pub fn take(&mut self, ticket: u64) -> Option<CaptureCompletion> {
        let index = self
            .completed
            .iter()
            .position(|result| result.ticket == ticket)?;
        self.completed.remove(index)
    }
}

/// Spawn a screenshot request and complete it through Bevy's render observer.
/// There is no filesystem polling and therefore no race between app shutdown
/// and a partially written last image.
pub fn request_capture(
    commands: &mut Commands,
    screenshot: Screenshot,
    request: CaptureWriteRequest,
    completions: &mut CaptureCompletions,
) -> u64 {
    let ticket = completions.allocate();
    let path = request.path.clone();
    commands.spawn(screenshot).observe(
        move |captured: On<ScreenshotCaptured>, mut completions: ResMut<CaptureCompletions>| {
            let result = write_capture_artifact(&captured.image, &request);
            let (comparison_failed, error) = match result {
                Ok(comparison_failed) => (comparison_failed, None),
                Err(error) => (false, Some(error)),
            };
            completions.completed.push_back(CaptureCompletion {
                ticket,
                path: path.clone(),
                comparison_failed,
                error,
            });
        },
    );
    ticket
}

fn write_capture_artifact(image: &Image, request: &CaptureWriteRequest) -> Result<bool, String> {
    if let Some(parent) = request.path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let rgb = image
        .clone()
        .try_into_dynamic()
        .map_err(|error| format!("cannot convert captured image: {error}"))?
        .to_rgb8();
    rgb.save_with_format(&request.path, image::ImageFormat::Png)
        .map_err(|error| format!("cannot save {}: {error}", request.path.display()))?;

    let comparison_attempt = request
        .comparison
        .as_ref()
        .map(|config| compare_or_update(&rgb, &request.path, config));
    let comparison = comparison_attempt
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .cloned();
    let comparison_error = comparison_attempt
        .as_ref()
        .and_then(|result| result.as_ref().err())
        .cloned();
    let comparison_failed = comparison.as_ref().is_some_and(|result| !result.passed);

    let mut metadata = request.metadata.clone();
    metadata.width = rgb.width();
    metadata.height = rgb.height();
    metadata.comparison = comparison;
    metadata.comparison_error = comparison_error.clone();
    let metadata_path = metadata_path(&request.path);
    let json = serde_json::to_string_pretty(&metadata)
        .map_err(|error| format!("cannot serialize capture metadata: {error}"))?;
    std::fs::write(&metadata_path, format!("{json}\n"))
        .map_err(|error| format!("cannot write {}: {error}", metadata_path.display()))?;
    if let Some(error) = comparison_error {
        return Err(error);
    }
    Ok(comparison_failed)
}

pub fn metadata_path(png: &Path) -> PathBuf {
    png.with_extension("capture.json")
}

fn compare_or_update(
    actual: &RgbImage,
    output_path: &Path,
    config: &CaptureComparisonConfig,
) -> Result<CaptureComparisonResult, String> {
    std::fs::create_dir_all(&config.baseline_dir).map_err(|error| {
        format!(
            "cannot create baseline directory {}: {error}",
            config.baseline_dir.display()
        )
    })?;
    let filename = output_path
        .file_name()
        .ok_or_else(|| format!("capture path has no filename: {}", output_path.display()))?;
    let baseline_path = config.baseline_dir.join(filename);
    if config.update_baselines {
        actual
            .save_with_format(&baseline_path, image::ImageFormat::Png)
            .map_err(|error| format!("cannot update {}: {error}", baseline_path.display()))?;
        return Ok(CaptureComparisonResult {
            baseline: baseline_path.display().to_string(),
            diff: None,
            baseline_updated: true,
            dimensions_match: true,
            mean_error: 0.0,
            changed_fraction: 0.0,
            passed: true,
        });
    }

    let baseline = image::ImageReader::open(&baseline_path)
        .map_err(|error| format!("cannot open baseline {}: {error}", baseline_path.display()))?
        .decode()
        .map_err(|error| {
            format!(
                "cannot decode baseline {}: {error}",
                baseline_path.display()
            )
        })?
        .to_rgb8();
    let dimensions_match = actual.dimensions() == baseline.dimensions();
    let (mean_error, changed_fraction, diff) = if dimensions_match {
        image_difference(actual, &baseline, config.pixel_threshold)
    } else {
        (1.0, 1.0, DynamicImage::ImageRgb8(actual.clone()).to_rgb8())
    };
    let passed = dimensions_match
        && mean_error <= config.maximum_mean_error
        && changed_fraction <= config.maximum_changed_fraction;
    let diff_path = (!passed).then(|| output_path.with_extension("diff.png"));
    if let Some(path) = &diff_path {
        diff.save_with_format(path, image::ImageFormat::Png)
            .map_err(|error| format!("cannot save diff {}: {error}", path.display()))?;
    }
    Ok(CaptureComparisonResult {
        baseline: baseline_path.display().to_string(),
        diff: diff_path.as_ref().map(|path| path.display().to_string()),
        baseline_updated: false,
        dimensions_match,
        mean_error,
        changed_fraction,
        passed,
    })
}

fn image_difference(
    actual: &RgbImage,
    baseline: &RgbImage,
    pixel_threshold: f32,
) -> (f32, f32, RgbImage) {
    let mut total_error = 0_u64;
    let mut changed = 0_u64;
    let mut diff = RgbImage::new(actual.width(), actual.height());
    let channel_threshold = (pixel_threshold.clamp(0.0, 1.0) * 255.0).round() as u8;
    for ((actual_pixel, baseline_pixel), diff_pixel) in actual
        .pixels()
        .zip(baseline.pixels())
        .zip(diff.pixels_mut())
    {
        let mut largest = 0_u8;
        for channel in 0..3 {
            let delta = actual_pixel[channel].abs_diff(baseline_pixel[channel]);
            total_error += u64::from(delta);
            largest = largest.max(delta);
        }
        if largest > channel_threshold {
            changed += 1;
        }
        *diff_pixel = image::Rgb([largest, 0, 255_u8.saturating_sub(largest)]);
    }
    let pixels = u64::from(actual.width()) * u64::from(actual.height());
    let mean_error = total_error as f32 / (pixels.max(1) * 3 * 255) as f32;
    let changed_fraction = changed as f32 / pixels.max(1) as f32;
    (mean_error, changed_fraction, diff)
}

pub fn git_commit() -> String {
    option_env!("FISTWORLD_GIT_COMMIT")
        .map(str::to_owned)
        .or_else(|| {
            std::process::Command::new("git")
                .args(["rev-parse", "--short=12", "HEAD"])
                .output()
                .ok()
                .filter(|output| output.status.success())
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .map(|value| value.trim().to_owned())
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenario_round_trip_keeps_semantic_contract() {
        let scenario = CaptureScenario::default();
        let ron = ron::ser::to_string_pretty(&scenario, ron::ser::PrettyConfig::default()).unwrap();
        let decoded: CaptureScenario = ron::from_str(&ron).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded.version, CAPTURE_SCENARIO_VERSION);
        assert_eq!(decoded.shots.len(), 1);
    }

    #[test]
    fn visual_difference_reports_changed_pixels() {
        let baseline = RgbImage::from_pixel(2, 1, image::Rgb([10, 10, 10]));
        let mut actual = baseline.clone();
        actual.put_pixel(1, 0, image::Rgb([110, 10, 10]));
        let (mean, changed, _) = image_difference(&actual, &baseline, 0.04);
        assert!(mean > 0.06 && mean < 0.07);
        assert_eq!(changed, 0.5);
    }

    #[test]
    fn rejects_scenarios_that_can_never_be_ready() {
        let mut scenario = CaptureScenario::default();
        scenario.readiness.minimum_frames = 10;
        scenario.readiness.maximum_frames = 9;
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn checked_in_scenarios_parse_and_validate() {
        for text in [
            include_str!("../../capture/scenarios/world-survey.ron"),
            include_str!("../../capture/scenarios/river-banks.ron"),
            include_str!("../../capture/scenarios/forest-floor.ron"),
            include_str!("../../capture/scenarios/isolated-fern.ron"),
            include_str!("../../capture/scenarios/lighting-readability.ron"),
            include_str!("../../capture/scenarios/ui-company.ron"),
        ] {
            let scenario: CaptureScenario = ron::from_str(text).unwrap();
            scenario.validate().unwrap();
        }
    }
}
