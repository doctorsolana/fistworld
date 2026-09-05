//! Game Client - Renders the world and handles player input.
//!
//! Thin wrapper: everything lives in `lib.rs` so `src/bin/capture.rs` can reuse it.

fn main() -> bevy::app::AppExit {
    client::run()
}
