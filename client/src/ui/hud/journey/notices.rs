//! Bounded local action history. Reading a message never sends gameplay intent.
use bevy::prelude::*;
use std::collections::VecDeque;

const LIMIT: usize = 3;

struct Entry {
    text: String,
    unread: bool,
}

#[derive(Resource, Default)]
pub(super) struct Notices {
    pub expanded: bool,
    sequence: u64,
    entries: VecDeque<Entry>,
}

impl Notices {
    pub fn has_seen(&self, sequence: u64) -> bool {
        self.sequence == sequence
    }

    pub fn observe(&mut self, sequence: u64, text: &str) {
        if sequence == self.sequence {
            return;
        }
        self.sequence = sequence;
        if text.is_empty() {
            return;
        }
        // Repeated orders move their existing message to the top instead of
        // flooding the tray with identical lines.
        self.entries.retain(|entry| entry.text != text);
        self.entries.push_front(Entry {
            text: text.into(),
            unread: true,
        });
        self.entries.truncate(LIMIT);
    }

    pub fn toggle(&mut self) {
        self.expanded = !self.expanded;
        if self.expanded {
            self.mark_read();
        }
    }

    pub fn mark_read(&mut self) {
        for entry in &mut self.entries {
            entry.unread = false;
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn unread(&self) -> usize {
        self.entries.iter().filter(|entry| entry.unread).count()
    }

    pub fn entry(&self, index: usize) -> Option<&str> {
        self.entries.get(index).map(|entry| entry.text.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_stay_bounded_coalesce_and_only_mark_read_when_opened() {
        let mut notices = Notices::default();
        for (i, text) in ["Moving", "At market", "No space", "Moving"]
            .iter()
            .enumerate()
        {
            notices.observe(i as u64 + 1, text);
        }
        assert!(!notices.expanded);
        assert_eq!(notices.unread(), 3);
        assert_eq!(notices.entry(0), Some("Moving"));
        assert_eq!(notices.entry(3), None);
        notices.toggle();
        assert_eq!(notices.unread(), 0);
        notices.observe(5, "New order");
        assert_eq!(
            notices.unread(),
            1,
            "expanded trays may be hidden behind a modal"
        );
        notices.mark_read();
        assert_eq!(notices.unread(), 0);
        notices.toggle();
        notices.observe(6, "New order");
        assert_eq!(notices.unread(), 1);
        notices.clear();
        notices.observe(6, "New order");
        assert_eq!(
            notices.entry(0),
            None,
            "cleared messages must not reappear each frame"
        );
    }
}
