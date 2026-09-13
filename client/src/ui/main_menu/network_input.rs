//! Launcher server-address editing and preset configuration.

use super::*;
use bevy::input_focus::{FocusCause, InputFocus};
use std::net::{IpAddr, Ipv6Addr, SocketAddr};

/// Load server presets synchronously (called during plugin build)
pub(super) fn load_server_presets_sync() -> (ServerPresets, ServerAddress) {
    // In dev, assets live under `client/assets/`.
    // In packaged builds (e.g. macOS .app), assets are bundled as `assets/` next to the executable.
    //
    // IMPORTANT: when double-clicking an app bundle, the *current working directory* is not reliable,
    // so we must resolve paths relative to the executable.
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            candidates.push(exe_dir.join("assets/servers.ron"));
        }
    }

    // Fallbacks (dev / running from repo)
    candidates.push(std::path::PathBuf::from("assets/servers.ron"));
    candidates.push(std::path::PathBuf::from("client/assets/servers.ron"));

    let found: Option<(std::path::PathBuf, String)> = candidates
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok().map(|c| (p.clone(), c)));

    let config: Option<ServerConfig> = found.as_ref().and_then(|(_p, c)| ron::from_str(c).ok());

    if let Some(config) = config {
        info!(
            "Loaded {} server presets from {}",
            config.servers.len(),
            found
                .as_ref()
                .and_then(|(p, _)| p.to_str())
                .unwrap_or("<unknown>")
        );

        // Set default server address from config
        let address = config
            .servers
            .get(config.default_index)
            .and_then(|entry| parse_server_address(&entry.ip, SERVER_PORT).ok());
        let selected_index = address.as_ref().map(|_| config.default_index);

        (
            ServerPresets {
                entries: config.servers,
                selected_index,
            },
            address.unwrap_or_default(),
        )
    } else {
        warn!(
            "Could not load servers.ron (tried {:?}), using defaults",
            candidates
        );
        (ServerPresets::default(), ServerAddress::default())
    }
}

/// Keep incomplete edits out of the address consumed by networking. Presets
/// remain `host + port`; a manual edit is committed only after validation.
#[derive(Resource, Default)]
pub(super) struct ServerAddressEditing {
    text: String,
    cursor: usize,
    anchor: usize,
    source: Option<(String, u16)>,
    selected_preset: Option<usize>,
    revision: u64,
    pub(super) submit_requested: bool,
}

impl ServerAddressEditing {
    fn sync_presets(&mut self, address: &ServerAddress, presets: &ServerPresets) {
        if self.selected_preset != presets.selected_index {
            self.selected_preset = presets.selected_index;
            // Choosing the original preset must also discard a custom draft
            // when the committed address itself did not change.
            if presets.selected_index.is_some() {
                self.source = None;
            }
        }
        self.sync_from(address);
    }

    pub(super) fn sync_from(&mut self, address: &ServerAddress) {
        if self
            .source
            .as_ref()
            .is_some_and(|(host, port)| host == &address.ip && *port == address.port)
        {
            return;
        }
        self.text = if address.ip.parse::<Ipv6Addr>().is_ok() {
            format!("[{}]:{}", address.ip, address.port)
        } else {
            format!("{}:{}", address.ip, address.port)
        };
        self.cursor = self.text.len();
        self.anchor = self.cursor;
        self.source = Some((address.ip.clone(), address.port));
        self.submit_requested = false;
        self.revision += 1;
    }

    pub(super) fn resolve(&self, default_port: u16) -> Result<ServerAddress, &'static str> {
        parse_server_address(&self.text, default_port)
    }

    fn selection(&self) -> std::ops::Range<usize> {
        self.cursor.min(self.anchor)..self.cursor.max(self.anchor)
    }

    fn remove_selection(&mut self) -> bool {
        let range = self.selection();
        if range.is_empty() {
            return false;
        }
        self.text.replace_range(range.clone(), "");
        self.cursor = range.start;
        self.anchor = self.cursor;
        true
    }

    fn insert(&mut self, text: &str) {
        let text = text.trim_matches(char::is_control);
        if text.is_empty() {
            return;
        }
        self.remove_selection();
        for ch in text.chars().filter(|ch| !ch.is_control()) {
            if self.text.len() + ch.len_utf8() > 320 {
                break;
            }
            self.text.insert(self.cursor, ch);
            self.cursor += ch.len_utf8();
        }
        self.anchor = self.cursor;
    }

    fn erase(&mut self, backward: bool) {
        if self.remove_selection() {
            return;
        }
        let range = if backward {
            self.text[..self.cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i)..self.cursor
        } else {
            self.cursor
                ..self.text[self.cursor..]
                    .chars()
                    .next()
                    .map_or(self.cursor, |ch| self.cursor + ch.len_utf8())
        };
        self.text.replace_range(range.clone(), "");
        self.cursor = range.start;
        self.anchor = self.cursor;
    }

    fn navigate(&mut self, key: &Key, shift: bool) {
        let selection = self.selection();
        self.cursor = match key {
            Key::Home => 0,
            Key::End => self.text.len(),
            Key::ArrowLeft if !shift && !selection.is_empty() => selection.start,
            Key::ArrowRight if !shift && !selection.is_empty() => selection.end,
            Key::ArrowLeft => self.text[..self.cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i),
            Key::ArrowRight => self.text[self.cursor..]
                .chars()
                .next()
                .map_or(self.cursor, |ch| self.cursor + ch.len_utf8()),
            _ => self.cursor,
        };
        if !shift {
            self.anchor = self.cursor;
        }
    }
}

/// Syntactic validation is immediate; DNS resolution remains asynchronous in
/// the connection flow. Never silently turn pasted URLs into a different host.
pub(super) fn parse_server_address(
    text: &str,
    default_port: u16,
) -> Result<ServerAddress, &'static str> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Enter a server hostname or IP address.");
    }
    if let Ok(address) = text.parse::<SocketAddr>() {
        if address.port() == 0 {
            return Err("Use a port between 1 and 65535.");
        }
        return Ok(ServerAddress {
            ip: address.ip().to_string(),
            port: address.port(),
        });
    }
    if let Ok(address) = text.parse::<IpAddr>() {
        return Ok(ServerAddress {
            ip: address.to_string(),
            port: default_port,
        });
    }
    let (host, port) = if let Some(bracketed) = text.strip_prefix('[') {
        let Some((host, suffix)) = bracketed.split_once(']') else {
            return Err("Use [IPv6 address]:port for an IPv6 server.");
        };
        if host.parse::<Ipv6Addr>().is_err() {
            return Err("Enter a valid IPv6 address inside the brackets.");
        }
        let port = if suffix.is_empty() {
            default_port
        } else {
            parse_port(suffix.strip_prefix(':').ok_or("Use [IPv6 address]:port.")?)?
        };
        return Ok(ServerAddress {
            ip: host.to_string(),
            port,
        });
    } else if let Some((host, port)) = text.rsplit_once(':') {
        (host, parse_port(port)?)
    } else {
        (text, default_port)
    };
    let hostname = host.strip_suffix('.').unwrap_or(host);
    if hostname.is_empty()
        || hostname.len() > 253
        || !hostname.is_ascii()
        || hostname.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == b'-')
        })
    {
        return Err("Use a hostname or IP address, with an optional :port.");
    }
    if hostname.contains('.')
        && hostname.bytes().all(|ch| ch.is_ascii_digit() || ch == b'.')
        && hostname.parse::<std::net::Ipv4Addr>().is_err()
    {
        return Err("Enter a valid IPv4 address.");
    }
    Ok(ServerAddress {
        ip: host.to_string(),
        port,
    })
}

fn parse_port(port: &str) -> Result<u16, &'static str> {
    if !port.bytes().all(|ch| ch.is_ascii_digit()) {
        return Err("Use a port between 1 and 65535.");
    }
    port.parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or("Use a port between 1 and 65535.")
}

pub(super) fn handle_ip_input_focus(
    mut fields: Query<(Entity, &Interaction, &mut IpInputField, &mut UiButtonStyle)>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut focus: Option<ResMut<InputFocus>>,
) {
    let clicked = fields.iter().find_map(|(entity, interaction, _, _)| {
        (*interaction == Interaction::Pressed).then_some(entity)
    });
    if let Some(focus) = focus.as_mut() {
        if let Some(entity) = clicked {
            focus.set(entity, FocusCause::Pressed);
        } else if mouse.just_pressed(MouseButton::Left)
            && focus.get().is_some_and(|entity| fields.contains(entity))
        {
            focus.clear();
        }
    }
    for (entity, _, mut field, mut style) in &mut fields {
        let focused = focus.as_ref().map_or_else(
            || clicked == Some(entity) || (!mouse.just_pressed(MouseButton::Left) && field.focused),
            |focus| focus.get() == Some(entity),
        );
        if field.focused != focused {
            field.focused = focused;
        }
        if style.focused != focused {
            style.focused = focused;
        }
        if !focused && style.selected {
            style.selected = false;
        }
    }
}

/// Enter finishes editing and requests Connect; the action system consumes
/// `submit_requested` after this system. Drain unfocused events to avoid replay.
pub(super) fn handle_ip_keyboard_input(
    mut fields: Query<(&mut IpInputField, &mut UiButtonStyle)>,
    address: Res<ServerAddress>,
    mut editing: ResMut<ServerAddressEditing>,
    mut events: MessageReader<KeyboardInput>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut presets: ResMut<ServerPresets>,
    mut focus: Option<ResMut<InputFocus>>,
) {
    editing.sync_presets(&address, &presets);
    let mut field = fields.iter_mut().find(|(field, _)| field.focused);
    let shortcut = keyboard.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    let shift = keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    for event in events.read() {
        let Some((field, style)) = field.as_mut() else {
            continue;
        };
        if !event.state.is_pressed() || !field.focused {
            continue;
        }
        let before = editing.text.clone();
        match &event.logical_key {
            Key::Character(key) if shortcut => match key.to_ascii_lowercase().as_str() {
                "a" => {
                    editing.anchor = 0;
                    editing.cursor = editing.text.len();
                }
                "v" => {
                    if let Ok(mut clipboard) = Clipboard::new() {
                        if let Ok(text) = clipboard.get_text() {
                            editing.insert(text.trim());
                        }
                    }
                }
                "c" | "x" => {
                    let selected = &editing.text[editing.selection()];
                    if !selected.is_empty() {
                        if let Ok(mut clipboard) = Clipboard::new() {
                            if clipboard.set_text(selected).is_ok() && key.eq_ignore_ascii_case("x")
                            {
                                editing.remove_selection();
                            }
                        }
                    }
                }
                _ => {}
            },
            Key::Backspace => editing.erase(true),
            Key::Delete => editing.erase(false),
            Key::ArrowLeft | Key::ArrowRight | Key::Home | Key::End => {
                editing.navigate(&event.logical_key, shift)
            }
            Key::Enter | Key::Escape => {
                editing.submit_requested = event.logical_key == Key::Enter;
                field.focused = false;
                style.focused = false;
                if let Some(focus) = focus.as_mut() {
                    focus.clear();
                }
            }
            Key::Character(key) => editing.insert(event.text.as_deref().unwrap_or(key.as_str())),
            _ => {}
        }
        if editing.text != before {
            presets.selected_index = None;
        }
        editing.revision += 1;
        style.selected = field.focused && !editing.selection().is_empty();
    }
}

pub(super) fn update_ip_display(
    address: Res<ServerAddress>,
    mut editing: ResMut<ServerAddressEditing>,
    fields: Query<&IpInputField>,
    mut displays: Query<(Entity, &mut Text), With<IpTextDisplay>>,
    presets: Res<ServerPresets>,
    time: Res<Time>,
    mut previous: Local<Option<(Entity, u64, bool)>>,
) {
    editing.sync_presets(&address, &presets);
    let caret = fields.iter().any(|field| field.focused) && time.elapsed_secs() % 1.0 < 0.55;
    for (entity, mut text) in &mut displays {
        let key = (entity, editing.revision, caret);
        if *previous == Some(key) {
            continue;
        }
        // Reuse Text's capacity; neither string formatting nor asset work runs
        // each frame. A caret transition changes the retained text at 2 Hz.
        text.0.clear();
        text.0.push_str(&editing.text[..editing.cursor]);
        if caret {
            text.0.push('|');
        }
        text.0.push_str(&editing.text[editing.cursor..]);
        *previous = Some(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::ButtonState;

    #[test]
    fn combined_address_supports_hostnames_ports_and_both_ip_families() {
        for (text, host, port) in [
            (" localhost ", "localhost", 5000),
            ("play.firstworld.test:6001", "play.firstworld.test", 6001),
            ("127.0.0.1:5000", "127.0.0.1", 5000),
            ("::1", "::1", 5000),
            ("[2001:db8::1]:6000", "2001:db8::1", 6000),
            ("[::1]", "::1", 5000),
            ("example.test.", "example.test.", 5000),
        ] {
            let address = parse_server_address(text, 5000).unwrap();
            assert_eq!(address.ip, host, "{text}");
            assert_eq!(address.port, port, "{text}");
        }
    }

    #[test]
    fn malformed_input_is_rejected_before_dns_or_connection() {
        for text in [
            "",
            " ",
            "https://example.test:5000",
            "host/name",
            "a..b",
            "-host",
            "host-",
            "host:0",
            "host:65536",
            "host:port",
            "host:+1",
            "host:",
            "[no-ip]:5000",
            "[::1]extra",
            "999.1.2.3",
            "host name",
            "höst",
        ] {
            assert!(parse_server_address(text, 5000).is_err(), "{text}");
        }
    }

    #[test]
    fn replace_selection_then_edit_port_keeps_committed_address_unchanged() {
        let address = ServerAddress::default();
        let mut edit = ServerAddressEditing::default();
        edit.sync_from(&address);
        edit.anchor = 0;
        edit.insert("play.example.test:5001");
        edit.navigate(&Key::ArrowLeft, true);
        edit.insert("2");
        assert_eq!(edit.text, "play.example.test:5002");
        assert_eq!(edit.resolve(5000).unwrap().port, 5002);
        assert_eq!(address.ip, "127.0.0.1");
        edit.navigate(&Key::Home, false);
        edit.erase(false);
        assert_eq!(edit.text, "lay.example.test:5002");
    }

    #[test]
    fn selecting_original_preset_discards_incomplete_manual_draft() {
        let address = ServerAddress::default();
        let mut presets = ServerPresets {
            selected_index: Some(0),
            ..default()
        };
        let mut edit = ServerAddressEditing::default();
        edit.sync_presets(&address, &presets);
        presets.selected_index = None;
        edit.sync_presets(&address, &presets);
        edit.anchor = 0;
        edit.insert("[unfinished");
        assert!(edit.resolve(5000).is_err());
        presets.selected_index = Some(0);
        edit.sync_presets(&address, &presets);
        assert_eq!(edit.text, "127.0.0.1:5000");
    }

    #[test]
    fn unfocused_events_do_not_replay_and_enter_requests_submission_once() {
        let mut app = App::new();
        app.init_resource::<ServerAddress>()
            .init_resource::<ServerAddressEditing>()
            .init_resource::<ServerPresets>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<KeyboardInput>()
            .add_systems(Update, handle_ip_keyboard_input);
        let entity = app
            .world_mut()
            .spawn((
                IpInputField { focused: false },
                UiButtonStyle::new(UiButtonVariant::Inverse),
            ))
            .id();
        let event = |key: Key| KeyboardInput {
            key_code: KeyCode::KeyA,
            logical_key: key,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: entity,
        };
        app.world_mut()
            .write_message(event(Key::Character("bad".into())));
        app.update();
        app.world_mut()
            .get_mut::<IpInputField>(entity)
            .unwrap()
            .focused = true;
        app.update();
        assert_eq!(
            app.world().resource::<ServerAddressEditing>().text,
            "127.0.0.1:5000"
        );
        app.world_mut().write_message(event(Key::Enter));
        app.update();
        assert!(
            app.world()
                .resource::<ServerAddressEditing>()
                .submit_requested
        );
        assert!(!app.world().get::<IpInputField>(entity).unwrap().focused);
        app.world_mut()
            .resource_mut::<ServerAddressEditing>()
            .submit_requested = false;
        app.update();
        assert!(
            !app.world()
                .resource::<ServerAddressEditing>()
                .submit_requested
        );
    }
}
