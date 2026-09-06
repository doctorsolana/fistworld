//! The game's bundled typefaces. No system-font fallback or per-screen loads.
use bevy::{asset::uuid_handle, prelude::*};

pub const DISPLAY: Handle<Font> = uuid_handle!("91abc482-7afc-4679-befa-829ade16fb14");
pub const BODY: Handle<Font> = uuid_handle!("b65eb4c7-6970-409b-8d88-b9df72a6fa29");

pub(super) fn install(app: &mut App) {
    bevy::asset::load_internal_binary_asset!(
        app,
        DISPLAY,
        "../../assets/fonts/Cinzel-Bold.ttf",
        |bytes: &[u8], _: String| Font::from_bytes(bytes.to_vec())
    );
    bevy::asset::load_internal_binary_asset!(
        app,
        BODY,
        "../../assets/fonts/MedievalSharp-Regular.ttf",
        |bytes: &[u8], _: String| Font::from_bytes(bytes.to_vec())
    );
}

pub fn body(size: f32) -> TextFont {
    TextFont::from_font_size(size.max(12.0)).with_font(BODY)
}

pub fn heading(size: f32) -> TextFont {
    TextFont::from_font_size(size).with_font(DISPLAY)
}

/// Existing screen scales use 17 px as their heading boundary. Keeping the
/// selection here makes every ledger, form and HUD use the same type hierarchy.
/// New layouts can use `body` / `heading` when their role overrides that scale.
pub fn text(size: f32) -> TextFont {
    if size >= 17.0 {
        heading(size)
    } else {
        body(size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_faces_register_in_bevys_font_database() {
        let mut app = App::new();
        app.init_resource::<Assets<Font>>();
        install(&mut app);
        let mut font_cx = bevy::text::FontCx::default();
        for handle in [DISPLAY, BODY] {
            let font = app.world().resource::<Assets<Font>>().get(&handle).unwrap();
            assert!(!font_cx
                .collection
                .register_fonts(font.data.clone(), None)
                .is_empty());
        }
    }
}
