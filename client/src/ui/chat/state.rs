//! Bounded local presentation state; accepted messages come only from the server.

use std::collections::VecDeque;

use bevy::prelude::*;
use shared::protocol::{ChatEvent, ChatRejectionReason, ChatSend, validate_chat_text};

use super::draft::Draft;

pub(crate) const HISTORY_LIMIT: usize = 100;
pub(super) const PREVIEW_HOLD_SECONDS: f64 = 10.0;
pub(super) const PREVIEW_FADE_SECONDS: f64 = 3.0;

#[derive(Clone, Debug)]
pub(crate) struct ChatLine {
    pub sequence: u64,
    pub sender: String,
    pub text: String,
    pub received_at: f64,
}

#[derive(Debug)]
pub(super) struct Pending {
    text: String,
    revision: u64,
    sent: bool,
}

#[derive(Resource, Default)]
pub(crate) struct ChatState {
    pub(crate) open: bool,
    pub(super) draft: Draft,
    pub(crate) messages: VecDeque<ChatLine>,
    pub(super) pending: Option<Pending>,
    pub(crate) feedback: Option<String>,
    pub(super) connection: Option<Entity>,
    last_sequence: u64,
    pub(super) history_revision: u64,
    pub(super) preview_visible: bool,
    pub(super) root_visible: bool,
    pub(super) focused: bool,
}

impl ChatState {
    /// Read-only capture evidence. No gameplay commands are issued here.
    pub(crate) fn diagnostics(&self) -> serde_json::Value {
        serde_json::json!({
            "open": self.open,
            "draft": self.draft.text,
            "messages": self.messages.iter().map(|line| serde_json::json!({
                "sequence": line.sequence, "sender": line.sender, "text": line.text,
            })).collect::<Vec<_>>(),
            "pending": self.pending.is_some(),
            "feedback": self.feedback,
            "preview_visible": self.preview_visible,
            "root_visible": self.root_visible,
            "history_count": self.messages.len(),
            "focused": self.focused,
        })
    }

    pub(super) fn queue_send(&mut self) -> bool {
        if self.pending.is_some() {
            self.feedback = Some("Waiting for your previous message to arrive.".into());
            return false;
        }
        if self.connection.is_none() {
            self.feedback = Some("Not connected. Your draft is kept.".into());
            return false;
        }
        let text = match validate_chat_text(&self.draft.text) {
            Ok(text) => text.to_owned(),
            Err(reason) => {
                self.feedback = Some(rejection_text(reason).into());
                return false;
            }
        };
        self.pending = Some(Pending {
            text,
            revision: self.draft.revision,
            sent: false,
        });
        self.feedback = None;
        self.open = false;
        true
    }

    pub(super) fn take_unsent(&mut self) -> Option<ChatSend> {
        let pending = self.pending.as_mut()?;
        if pending.sent {
            return None;
        }
        pending.sent = true;
        Some(ChatSend {
            text: pending.text.clone(),
        })
    }

    /// The production receiver and deterministic presentation fixture share this path.
    /// `local_name` is the accepted account name, never a guessed world-person name.
    pub(crate) fn receive(&mut self, event: ChatEvent, now: f64, local_name: &str) {
        match event {
            ChatEvent::Message {
                sequence,
                sender,
                text,
            } => {
                if sequence <= self.last_sequence {
                    return;
                }
                self.last_sequence = sequence;
                // Profiles are keyed by Unicode lowercase names on the server.
                // A reconnect may use different casing from the saved display name.
                if self
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.text == text)
                    && sender.to_lowercase() == local_name.trim().to_lowercase()
                {
                    let pending = self.pending.take().unwrap();
                    if self.draft.revision == pending.revision {
                        self.draft.clear();
                    }
                    self.feedback = None;
                }
                self.messages.push_back(ChatLine {
                    sequence,
                    sender,
                    text,
                    received_at: now,
                });
                while self.messages.len() > HISTORY_LIMIT {
                    self.messages.pop_front();
                }
                self.history_revision = self.history_revision.wrapping_add(1);
            }
            ChatEvent::Rejected { reason } => {
                self.pending = None;
                self.feedback = Some(rejection_text(reason).into());
            }
        }
    }

    pub(super) fn set_connection(&mut self, connection: Option<Entity>) {
        if self.connection == connection {
            return;
        }
        let interrupted = self.pending.take().is_some();
        self.connection = connection;
        self.messages.clear();
        self.last_sequence = 0;
        self.history_revision = self.history_revision.wrapping_add(1);
        self.open = false;
        self.focused = false;
        self.preview_visible = false;
        self.root_visible = false;
        self.feedback =
            interrupted.then(|| "Connection changed. Your draft is kept; press T to retry.".into());
    }

    pub(super) fn reset_session(&mut self) {
        let draft = std::mem::take(&mut self.draft);
        *self = Self { draft, ..default() };
    }
}

pub(super) fn rejection_text(reason: ChatRejectionReason) -> &'static str {
    match reason {
        ChatRejectionReason::Empty => "Write a message first.",
        ChatRejectionReason::TooLong => "Keep your message within 280 characters and 1024 bytes.",
        ChatRejectionReason::InvalidCharacters => {
            "Use one line of text without control characters."
        }
        ChatRejectionReason::RateLimited => {
            "A moment, please. Wait briefly, then press T to retry."
        }
    }
}

pub(super) fn preview_alpha(received_at: f64, now: f64) -> f32 {
    (1.0 - ((now - received_at - PREVIEW_HOLD_SECONDS) / PREVIEW_FADE_SECONDS).max(0.0))
        .clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(sequence: u64, sender: &str, text: &str) -> ChatEvent {
        ChatEvent::Message {
            sequence,
            sender: sender.into(),
            text: text.into(),
        }
    }

    #[test]
    fn history_is_bounded_ordered_and_has_no_optimistic_duplicate() {
        let mut state = ChatState::default();
        state.set_connection(Some(Entity::from_bits(1)));
        state.draft.insert("hello").unwrap();
        assert!(state.queue_send());
        assert!(state.messages.is_empty());
        assert_eq!(
            state.take_unsent(),
            Some(ChatSend {
                text: "hello".into()
            })
        );
        assert_eq!(state.take_unsent(), None);
        for sequence in 1..=105 {
            state.receive(message(sequence, "Alice", "hello"), 0.0, "Alice");
        }
        state.receive(message(105, "Alice", "duplicate"), 0.0, "Alice");
        state.receive(message(10, "Alice", "old"), 0.0, "Alice");
        assert_eq!(state.messages.len(), HISTORY_LIMIT);
        assert_eq!(state.messages.front().unwrap().sequence, 6);
        assert_eq!(state.messages.back().unwrap().sequence, 105);
        assert!(state.draft.text.is_empty());
        assert!(state.pending.is_none());
    }

    #[test]
    fn echo_does_not_clear_new_typing_and_rejection_keeps_the_draft() {
        let mut state = ChatState::default();
        state.set_connection(Some(Entity::from_bits(1)));
        state.draft.insert("first").unwrap();
        assert!(state.queue_send());
        assert!(!state.queue_send());
        state.draft.insert(" and more").unwrap();
        state.receive(message(1, "Alice", "first"), 0.0, "Alice");
        assert_eq!(state.draft.text, "first and more");
        assert!(state.queue_send());
        state.receive(
            ChatEvent::Rejected {
                reason: ChatRejectionReason::RateLimited,
            },
            0.0,
            "Alice",
        );
        assert_eq!(state.draft.text, "first and more");
        assert!(state.pending.is_none());
        assert!(state.feedback.as_ref().unwrap().contains("Wait briefly"));
    }

    #[test]
    fn reconnect_restarts_sequence_without_resending_an_unacknowledged_draft() {
        let mut state = ChatState::default();
        state.set_connection(Some(Entity::from_bits(1)));
        state.receive(message(500, "Alice", "old session"), 0.0, "Alice");
        state.draft.insert("kept").unwrap();
        assert!(state.queue_send());
        state.set_connection(None);
        assert_eq!(state.draft.text, "kept");
        assert!(state.take_unsent().is_none());
        state.set_connection(Some(Entity::from_bits(2)));
        state.receive(message(1, "Bob", "new session"), 0.0, "Alice");
        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.messages[0].sequence, 1);
    }

    #[test]
    fn recased_account_echo_matches_the_saved_authoritative_display_name() {
        let mut state = ChatState::default();
        state.set_connection(Some(Entity::from_bits(1)));
        state.draft.insert("home again").unwrap();
        assert!(state.queue_send());
        state.receive(message(1, "Ålice", "home again"), 0.0, "ÅLICE");
        assert!(state.pending.is_none());
        assert!(state.draft.text.is_empty());
        assert_eq!(state.messages[0].sender, "Ålice");
    }

    #[test]
    fn compact_messages_fade_on_real_elapsed_time() {
        assert_eq!(preview_alpha(20.0, 29.0), 1.0);
        assert_eq!(preview_alpha(20.0, 31.5), 0.5);
        assert_eq!(preview_alpha(20.0, 33.0), 0.0);
    }
}
