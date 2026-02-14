use std::time::Duration;

pub const SERVER_PORT: u16 = 5000;
pub const SERVER_ADDR: &str = "127.0.0.1";
pub const PROTOCOL_ID: u64 = 0x1234567890ABCDF1;
pub const NETCODE_CLIENT_TIMEOUT_SECS: i32 = 10;
pub const NETCODE_TOKEN_EXPIRE_SECS: i32 = 30;

/// Get the address the server should bind to.
///
/// On Fly.io, UDP services should bind to `fly-global-services` for correct routing.
/// Locally, bind to `0.0.0.0`.
pub fn get_server_bind_addr() -> &'static str {
    if std::env::var("FLY_APP_NAME").is_ok() {
        "fly-global-services"
    } else {
        "0.0.0.0"
    }
}

/// Shared private key for local development (use proper key management in production!).
pub const PRIVATE_KEY: [u8; 32] = [
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
    0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
];

/// Fixed timestep for physics/game logic (60 Hz).
pub const FIXED_TIMESTEP_HZ: f64 = 60.0;

/// Tick duration for lightyear plugins.
pub fn tick_duration() -> Duration {
    Duration::from_secs_f64(1.0 / FIXED_TIMESTEP_HZ)
}
