//! input systems.

use super::*;

pub(super) fn handle_text_input(
    mut name_input: ResMut<PlayerNameInput>,
    mut key_events: MessageReader<KeyboardInput>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut display_query: Query<&mut Text, With<NameInputDisplay>>,
) {
    // Don't accept input if already submitted
    if name_input.submitted {
        return;
    }

    // Handle backspace
    if keyboard.just_pressed(KeyCode::Backspace) {
        name_input.name.pop();
    }

    // Handle character input from keyboard events
    for event in key_events.read() {
        if !event.state.is_pressed() {
            continue;
        }

        // Convert KeyCode to character
        let c_opt = match event.key_code {
            KeyCode::KeyA => Some('a'),
            KeyCode::KeyB => Some('b'),
            KeyCode::KeyC => Some('c'),
            KeyCode::KeyD => Some('d'),
            KeyCode::KeyE => Some('e'),
            KeyCode::KeyF => Some('f'),
            KeyCode::KeyG => Some('g'),
            KeyCode::KeyH => Some('h'),
            KeyCode::KeyI => Some('i'),
            KeyCode::KeyJ => Some('j'),
            KeyCode::KeyK => Some('k'),
            KeyCode::KeyL => Some('l'),
            KeyCode::KeyM => Some('m'),
            KeyCode::KeyN => Some('n'),
            KeyCode::KeyO => Some('o'),
            KeyCode::KeyP => Some('p'),
            KeyCode::KeyQ => Some('q'),
            KeyCode::KeyR => Some('r'),
            KeyCode::KeyS => Some('s'),
            KeyCode::KeyT => Some('t'),
            KeyCode::KeyU => Some('u'),
            KeyCode::KeyV => Some('v'),
            KeyCode::KeyW => Some('w'),
            KeyCode::KeyX => Some('x'),
            KeyCode::KeyY => Some('y'),
            KeyCode::KeyZ => Some('z'),
            KeyCode::Digit0 => Some('0'),
            KeyCode::Digit1 => Some('1'),
            KeyCode::Digit2 => Some('2'),
            KeyCode::Digit3 => Some('3'),
            KeyCode::Digit4 => Some('4'),
            KeyCode::Digit5 => Some('5'),
            KeyCode::Digit6 => Some('6'),
            KeyCode::Digit7 => Some('7'),
            KeyCode::Digit8 => Some('8'),
            KeyCode::Digit9 => Some('9'),
            KeyCode::Minus => Some('-'),
            _ => None,
        };

        if let Some(c) = c_opt {
            // Limit to 16 characters
            if name_input.name.len() < 16 {
                name_input.name.push(c);
            }
        }
    }

    // Update display
    for mut text in display_query.iter_mut() {
        text.0 = if name_input.name.is_empty() {
            "_".to_string()
        } else {
            format!("{}_", name_input.name)
        };
    }
}
