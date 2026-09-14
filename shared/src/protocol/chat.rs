//! Ephemeral, server-wide plaintext chat. Identity is supplied by the server.

use serde::{Deserialize, Serialize};

pub const CHAT_MAX_CHARACTERS: usize = 280;
pub const CHAT_MAX_BYTES: usize = 1024;

/// Client -> server. A client cannot provide the sender's account or name.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct ChatSend {
    #[serde(deserialize_with = "deserialize_chat_text")]
    pub text: String,
}

/// Bincode strings and byte sequences both encode a byte length followed by
/// those bytes. Read that sequence ourselves to reject oversized declarations
/// before allocating a String, rather than relying only on handler validation.
/// Human-readable tooling retains the normal {"text":"..."} representation.
fn deserialize_chat_text<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    use serde::de::{Error, SeqAccess, Visitor};
    if deserializer.is_human_readable() {
        let text = String::deserialize(deserializer)?;
        return if text.len() <= CHAT_MAX_BYTES {
            Ok(text)
        } else {
            Err(D::Error::custom("chat text exceeds byte limit"))
        };
    }
    struct BoundedChatBytes;
    impl<'de> Visitor<'de> for BoundedChatBytes {
        type Value = String;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("at most 1024 UTF-8 bytes")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<String, A::Error> {
            if seq.size_hint().is_some_and(|len| len > CHAT_MAX_BYTES) {
                return Err(A::Error::custom("chat text exceeds byte limit"));
            }
            let mut bytes = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(CHAT_MAX_BYTES));
            while let Some(byte) = seq.next_element::<u8>()? {
                if bytes.len() == CHAT_MAX_BYTES {
                    return Err(A::Error::custom("chat text exceeds byte limit"));
                }
                bytes.push(byte);
            }
            String::from_utf8(bytes).map_err(|_| A::Error::custom("chat text is not UTF-8"))
        }
    }
    deserializer.deserialize_seq(BoundedChatBytes)
}

/// Server -> client, including the sender's own accepted message.
/// Sequence numbers increase within this server process; there is no history replay.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum ChatEvent {
    Message {
        sequence: u64,
        sender: String,
        text: String,
    },
    Rejected {
        reason: ChatRejectionReason,
    },
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
pub enum ChatRejectionReason {
    Empty,
    TooLong,
    InvalidCharacters,
    RateLimited,
}

/// A separate ordered reliable stream keeps chat independent of gameplay replies.
pub struct ChatChannel;

/// Validate before copying. Both limits apply; the byte check bounds Unicode work.
/// Chat is one plaintext line: control/line separators and directional overrides
/// are rejected, while ordinary Unicode and emoji joiners remain intact.
pub fn validate_chat_text(text: &str) -> Result<&str, ChatRejectionReason> {
    if text.len() > CHAT_MAX_BYTES {
        return Err(ChatRejectionReason::TooLong);
    }
    if text.chars().any(|c| {
        c.is_control()
            || matches!(c, '\u{061c}' | '\u{200b}' | '\u{200e}' | '\u{200f}'
                | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{206f}' | '\u{feff}')
    }) {
        return Err(ChatRejectionReason::InvalidCharacters);
    }
    let text = text.trim();
    if text.is_empty() {
        return Err(ChatRejectionReason::Empty);
    }
    if text.chars().count() > CHAT_MAX_CHARACTERS {
        return Err(ChatRejectionReason::TooLong);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_wire_messages_roundtrip() {
        let request = ChatSend {
            text: "Olá 世界 👩‍🌾".into(),
        };
        assert_eq!(
            bincode::deserialize::<ChatSend>(&bincode::serialize(&request).unwrap()).unwrap(),
            request
        );
        for event in [
            ChatEvent::Message {
                sequence: 42,
                sender: "Alice".into(),
                text: request.text,
            },
            ChatEvent::Rejected {
                reason: ChatRejectionReason::Empty,
            },
            ChatEvent::Rejected {
                reason: ChatRejectionReason::TooLong,
            },
            ChatEvent::Rejected {
                reason: ChatRejectionReason::InvalidCharacters,
            },
            ChatEvent::Rejected {
                reason: ChatRejectionReason::RateLimited,
            },
        ] {
            assert_eq!(
                bincode::deserialize::<ChatEvent>(&bincode::serialize(&event).unwrap()).unwrap(),
                event
            );
        }
    }

    #[test]
    fn chat_limits_count_unicode_characters_and_bytes_without_truncation() {
        let boundary = "界".repeat(CHAT_MAX_CHARACTERS);
        assert_eq!(validate_chat_text(&boundary), Ok(boundary.as_str()));
        assert_eq!(
            validate_chat_text(&"a".repeat(CHAT_MAX_CHARACTERS + 1)),
            Err(ChatRejectionReason::TooLong)
        );
        assert_eq!(
            validate_chat_text(&"🙂".repeat(CHAT_MAX_CHARACTERS)),
            Err(ChatRejectionReason::TooLong)
        );
        assert_eq!(
            validate_chat_text(&" ".repeat(CHAT_MAX_BYTES + 1)),
            Err(ChatRejectionReason::TooLong)
        );
    }

    #[test]
    fn chat_binary_decoder_rejects_oversize_before_reading_or_allocating_the_body() {
        // The only field is a length-prefixed string. A huge length without a
        // body must hit our bound, not allocate or attempt to read that body.
        let bytes = u64::MAX.to_le_bytes();
        let error = bincode::deserialize::<ChatSend>(&bytes)
            .unwrap_err()
            .to_string();
        assert!(error.contains("chat text exceeds byte limit"), "{error}");
        let oversized = ChatSend {
            text: "x".repeat(CHAT_MAX_BYTES + 1),
        };
        assert!(
            bincode::deserialize::<ChatSend>(&bincode::serialize(&oversized).unwrap()).is_err()
        );
        let malformed_utf8 = [2u64.to_le_bytes().as_slice(), &[0xff, 0xfe]].concat();
        assert!(bincode::deserialize::<ChatSend>(&malformed_utf8).is_err());
    }

    #[test]
    fn chat_is_one_plaintext_line_and_preserves_international_text() {
        assert_eq!(
            validate_chat_text("  café 世界 👩‍🌾 <hello>  "),
            Ok("café 世界 👩‍🌾 <hello>")
        );
        assert_eq!(
            validate_chat_text("\u{2003} "),
            Err(ChatRejectionReason::Empty)
        );
        for control in [
            '\n', '\r', '\t', '\0', '\u{7f}', '\u{85}', '\u{2028}', '\u{202e}', '\u{2066}',
            '\u{feff}',
        ] {
            assert_eq!(
                validate_chat_text(&format!("hello{control}world")),
                Err(ChatRejectionReason::InvalidCharacters)
            );
        }
    }
}
