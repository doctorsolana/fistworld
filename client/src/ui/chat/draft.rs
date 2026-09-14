//! UTF-8-safe single-line editing. Limits and text safety match the wire contract.

use bevy::input::keyboard::Key;
use shared::protocol::{
    CHAT_MAX_BYTES, CHAT_MAX_CHARACTERS, ChatRejectionReason, validate_chat_text,
};

#[derive(Default)]
pub(super) struct Draft {
    pub text: String,
    pub cursor: usize,
    pub anchor: usize,
    pub revision: u64,
}

impl Draft {
    pub fn selection(&self) -> std::ops::Range<usize> {
        self.cursor.min(self.anchor)..self.cursor.max(self.anchor)
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.anchor = 0;
        self.revision = self.revision.wrapping_add(1);
    }

    /// Reject an invalid paste as a whole, retaining the previous draft/selection.
    pub fn insert(&mut self, text: &str) -> Result<(), ChatRejectionReason> {
        let selection = self.selection();
        if text.len() > CHAT_MAX_BYTES
            || self.text.len() - selection.len() > CHAT_MAX_BYTES.saturating_sub(text.len())
        {
            return Err(ChatRejectionReason::TooLong);
        }
        let mut next = self.text.clone();
        next.replace_range(selection.clone(), text);
        match validate_chat_text(&next) {
            Ok(_) | Err(ChatRejectionReason::Empty) => (),
            Err(reason) => return Err(reason),
        }
        if next.chars().count() > CHAT_MAX_CHARACTERS {
            return Err(ChatRejectionReason::TooLong);
        }
        self.text = next;
        self.cursor = selection.start + text.len();
        self.anchor = self.cursor;
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    pub fn erase(&mut self, backward: bool) {
        let selected = self.selection();
        let range = if !selected.is_empty() {
            selected
        } else if backward {
            self.previous()..self.cursor
        } else {
            self.cursor..self.next()
        };
        if range.is_empty() {
            return;
        }
        self.cursor = range.start;
        self.anchor = self.cursor;
        self.text.replace_range(range, "");
        self.revision = self.revision.wrapping_add(1);
    }

    fn previous(&self) -> usize {
        self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }
    fn next(&self) -> usize {
        self.text[self.cursor..]
            .chars()
            .next()
            .map_or(self.cursor, |c| self.cursor + c.len_utf8())
    }
    pub fn navigate(&mut self, key: &Key, shift: bool) {
        let range = self.selection();
        self.cursor = match key {
            Key::Home => 0,
            Key::End => self.text.len(),
            Key::ArrowLeft if !shift && !range.is_empty() => range.start,
            Key::ArrowRight if !shift && !range.is_empty() => range.end,
            Key::ArrowLeft => self.previous(),
            Key::ArrowRight => self.next(),
            _ => self.cursor,
        };
        if !shift {
            self.anchor = self.cursor;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_selection_editing_keeps_character_boundaries() {
        let mut draft = Draft::default();
        draft.insert("café 世界 👩‍🌾").unwrap();
        draft.navigate(&Key::Home, false);
        for _ in 0..4 {
            draft.navigate(&Key::ArrowRight, true);
        }
        draft.insert("Olé").unwrap();
        assert_eq!(draft.text, "Olé 世界 👩‍🌾");
        draft.erase(true);
        assert_eq!(draft.text, "Ol 世界 👩‍🌾");
        draft.erase(false);
        assert_eq!(draft.text, "Ol世界 👩‍🌾");
    }

    #[test]
    fn unsafe_or_oversized_paste_preserves_existing_draft_and_selection() {
        let mut draft = Draft::default();
        draft.insert("kept").unwrap();
        draft.anchor = 0;
        for bad in ["line\nbreak".to_owned(), "x".repeat(281), "🙂".repeat(280)] {
            assert!(draft.insert(&bad).is_err());
            assert_eq!(draft.text, "kept");
            assert_eq!(draft.selection(), 0..4);
        }
        draft.insert(" ").unwrap();
        assert_eq!(draft.text, " ");
    }
}
