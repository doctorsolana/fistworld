//! network input systems.

use super::*;

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
        let ip = config
            .servers
            .get(config.default_index)
            .map(|e| e.ip.clone())
            .unwrap_or_else(|| "127.0.0.1".to_string());

        (
            ServerPresets {
                entries: config.servers,
                selected_index: Some(config.default_index),
            },
            ServerAddress {
                ip,
                port: SERVER_PORT,
            },
        )
    } else {
        warn!(
            "Could not load servers.ron (tried {:?}), using defaults",
            candidates
        );
        (ServerPresets::default(), ServerAddress::default())
    }
}

/// Handle clicking on the IP input field to focus/unfocus
pub(super) fn handle_ip_input_focus(
    mut input_fields: Query<(&Interaction, &mut IpInputField, &mut UiButtonStyle)>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut presets: ResMut<ServerPresets>,
) {
    let mut any_clicked = false;

    for (interaction, mut field, mut style) in input_fields.iter_mut() {
        if *interaction == Interaction::Pressed {
            field.focused = true;
            any_clicked = true;
            style.focused = true;
            // When manually editing, deselect preset
            presets.selected_index = None;
        }
    }

    // Unfocus if clicked elsewhere
    if mouse_button.just_pressed(MouseButton::Left) && !any_clicked {
        for (_, mut field, mut style) in input_fields.iter_mut() {
            if field.focused {
                field.focused = false;
                style.focused = false;
            }
        }
    }
}

/// Handle keyboard input when IP field is focused
pub(super) fn handle_ip_keyboard_input(
    mut input_fields: Query<(&mut IpInputField, &mut UiButtonStyle)>,
    mut server_address: ResMut<ServerAddress>,
    mut keyboard_events: MessageReader<KeyboardInput>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut presets: ResMut<ServerPresets>,
) {
    let Some((mut field, mut style)) = input_fields.iter_mut().find(|(field, _)| field.focused)
    else {
        return;
    };

    // Check for Ctrl modifier (for paste)
    let ctrl_held =
        keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);

    for event in keyboard_events.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }

        match &event.logical_key {
            Key::Backspace => {
                // Remove last character from IP
                if !server_address.ip.is_empty() {
                    server_address.ip.pop();
                    // Deselect preset when editing manually
                    presets.selected_index = None;
                }
            }
            Key::Escape | Key::Enter => {
                // Unfocus on escape or enter
                field.focused = false;
                style.focused = false;
            }
            Key::Character(c) => {
                let c_str = c.as_str();

                // Handle Ctrl+V paste
                if ctrl_held && (c_str == "v" || c_str == "V") {
                    if let Ok(mut clipboard) = Clipboard::new() {
                        if let Ok(text) = clipboard.get_text() {
                            // Filter to valid IP/hostname characters and append
                            for ch in text.chars() {
                                if (ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')
                                    && server_address.ip.len() < 63
                                {
                                    server_address.ip.push(ch);
                                }
                            }
                            // Deselect preset when pasting
                            presets.selected_index = None;
                            info!("Pasted IP from clipboard: {}", server_address.ip);
                        }
                    }
                    continue;
                }

                // Allow valid IP/hostname characters: alphanumeric, dots, dashes
                if c_str.len() == 1 {
                    let ch = c_str.chars().next().unwrap();
                    if (ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')
                        && server_address.ip.len() < 63
                    {
                        server_address.ip.push(ch);
                        // Deselect preset when editing manually
                        presets.selected_index = None;
                    }
                }
            }
            _ => {}
        }
    }
}

/// Update the displayed IP text
pub(super) fn update_ip_display(
    server_address: Res<ServerAddress>,
    input_fields: Query<&IpInputField>,
    mut text_query: Query<&mut Text, With<IpTextDisplay>>,
    time: Res<Time>,
    mut cursor_timer: Local<f32>,
) {
    let is_focused = input_fields.iter().any(|f| f.focused);

    *cursor_timer += time.delta_secs();
    let show_cursor = is_focused && (*cursor_timer % 1.0) < 0.5;

    for mut text in text_query.iter_mut() {
        let display = if server_address.ip.is_empty() {
            format!("_:{}", server_address.port)
        } else {
            format!("{}:{}", server_address.ip, server_address.port)
        };

        let cursor = if show_cursor { "|" } else { "" };
        **text = format!("{}{}", display, cursor);
    }
}
