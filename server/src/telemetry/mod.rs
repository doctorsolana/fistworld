//! Telemetry and diagnostics domain.
//!
//! Responsibilities:
//! - Fixed-tick performance instrumentation and logging.
//!
//! Dependency notes:
//! - May read cross-domain state for observability.
//! - Should not mutate gameplay authority/state transitions.

pub mod perf;
pub mod network;

use std::sync::OnceLock;

/// Enable verbose per-event hot-path logs with `CITYSIM_SERVER_HOTLOG=1`.
pub fn hotlog_enabled() -> bool {
    static HOTLOG_ENABLED: OnceLock<bool> = OnceLock::new();
    *HOTLOG_ENABLED.get_or_init(|| {
        std::env::var("CITYSIM_SERVER_HOTLOG")
            .map(|value| {
                let normalized = value.trim().to_ascii_lowercase();
                !(normalized == "0" || normalized == "false" || normalized == "off")
            })
            .unwrap_or(false)
    })
}
