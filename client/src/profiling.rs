//! Lightweight runtime profiling switches.

pub fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|value| {
            let value = value.trim().to_ascii_lowercase();
            matches!(value.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

pub fn hitch_profiling_enabled() -> bool {
    env_flag("FISTFORCE_PROFILE_HITCHES") || env_flag("CITYSIM_PROFILE_HITCHES")
}

/// True when a switch was set to an explicit off value (`0`/`false`/`off`/`no`).
/// Absent and empty values are "not off", not "off": these are kill switches,
/// so only an explicit value may turn a subsystem off.
pub fn env_flag_off(name: &str) -> bool {
    std::env::var(name)
        .map(|value| {
            let value = value.trim().to_ascii_lowercase();
            matches!(value.as_str(), "0" | "false" | "off" | "no")
        })
        .unwrap_or(false)
}

/// A subsystem that is on unless its switch is explicitly off.
pub fn env_enabled_by_default(name: &str) -> bool {
    !env_flag_off(name)
}

pub fn env_f32(name: &str, fallback: f32) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .unwrap_or(fallback)
}
