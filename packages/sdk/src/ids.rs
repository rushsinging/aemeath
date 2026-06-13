//! UUIDv7 newtypes for internal chat, turn, and tool call IDs.

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Deterministic UUIDv7 from a string (namespace-based, for test stability).
fn deterministic_uuidv7(s: &str) -> Uuid {
    let namespace = Uuid::from_bytes([
        0xa1, 0x7e, 0x0a, 0x7e, 0x0a, 0x7e, 0x0a, 0x7e, 0xa1, 0x7e, 0x0a, 0x7e, 0x0a, 0x7e, 0x0a,
        0x7e,
    ]);
    let base = Uuid::new_v5(&namespace, s.as_bytes());
    let mut bytes = *base.as_bytes();
    // Set version to 7 (bits 48-51 of time_hi_and_version)
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    // Set variant to RFC 4122 (bits 6-7 of clock_seq_hi_and_reserved)
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

/// Error returned when parsing a non-UUIDv7 string as an internal ID.
#[derive(Debug, Clone, thiserror::Error)]
pub enum IdParseError {
    #[error("无效的 UUID 格式: {0}")]
    InvalidUuid(String),
    #[error("UUID 不是 version 7: {0}")]
    NotVersion7(String),
}

/// Internal chat ID (UUIDv7).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChatId(Uuid);

impl ChatId {
    /// Generate a new UUIDv7 chat ID.
    pub fn new_v7() -> Self {
        Self(Uuid::now_v7())
    }

    /// Create a ChatId from a legacy string or generate new UUIDv7.
    /// Alias for `from_legacy_or_new` — use `new_v7()` for fresh IDs.
    pub fn new(s: &str) -> Self {
        Self::from_legacy_or_new(s)
    }

    /// Parse a UUIDv7 string as a ChatId.
    pub fn parse_uuid7(s: &str) -> Result<Self, IdParseError> {
        let uuid = Uuid::parse_str(s).map_err(|_| IdParseError::InvalidUuid(s.to_string()))?;
        if uuid.get_version_num() != 7 {
            return Err(IdParseError::NotVersion7(s.to_string()));
        }
        Ok(Self(uuid))
    }

    /// Convert from legacy string or generate new UUIDv7.
    /// If the input is a valid UUIDv7, preserves it.
    /// Otherwise, deterministically generates a UUIDv7 from the input string.
    pub fn from_legacy_or_new(s: &str) -> Self {
        Self::parse_uuid7(s).unwrap_or_else(|_| Self(deterministic_uuidv7(s)))
    }

    /// Get the inner UUID.
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    /// Get string representation.
    pub fn as_str(&self) -> String {
        self.0.to_string()
    }
}

impl fmt::Display for ChatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for ChatId {
    fn as_ref(&self) -> &str {
        // Note: This leaks, but acceptable for ID types
        Box::leak(self.0.to_string().into_boxed_str())
    }
}

/// Internal turn ID (UUIDv7).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChatTurnId(Uuid);

impl ChatTurnId {
    pub fn new_v7() -> Self {
        Self(Uuid::now_v7())
    }

    /// Create a ChatTurnId from a legacy string or generate new UUIDv7.
    /// Alias for `from_legacy_or_new` — use `new_v7()` for fresh IDs.
    pub fn new(s: &str) -> Self {
        Self::from_legacy_or_new(s)
    }

    pub fn parse_uuid7(s: &str) -> Result<Self, IdParseError> {
        let uuid = Uuid::parse_str(s).map_err(|_| IdParseError::InvalidUuid(s.to_string()))?;
        if uuid.get_version_num() != 7 {
            return Err(IdParseError::NotVersion7(s.to_string()));
        }
        Ok(Self(uuid))
    }

    pub fn from_legacy_or_new(s: &str) -> Self {
        Self::parse_uuid7(s).unwrap_or_else(|_| Self(deterministic_uuidv7(s)))
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    pub fn as_str(&self) -> String {
        self.0.to_string()
    }
}

impl fmt::Display for ChatTurnId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for ChatTurnId {
    fn as_ref(&self) -> &str {
        Box::leak(self.0.to_string().into_boxed_str())
    }
}

/// Internal tool call ID (UUIDv7).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ToolCallId(Uuid);

impl ToolCallId {
    pub fn new_v7() -> Self {
        Self(Uuid::now_v7())
    }

    /// Create a ToolCallId from a legacy string or generate new UUIDv7.
    /// Alias for `from_legacy_or_new` — use `new_v7()` for fresh IDs.
    pub fn new(s: String) -> Self {
        Self::from_legacy_or_new(&s)
    }

    pub fn parse_uuid7(s: &str) -> Result<Self, IdParseError> {
        let uuid = Uuid::parse_str(s).map_err(|_| IdParseError::InvalidUuid(s.to_string()))?;
        if uuid.get_version_num() != 7 {
            return Err(IdParseError::NotVersion7(s.to_string()));
        }
        Ok(Self(uuid))
    }

    pub fn from_legacy_or_new(s: &str) -> Self {
        Self::parse_uuid7(s).unwrap_or_else(|_| Self(deterministic_uuidv7(s)))
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    pub fn as_str(&self) -> String {
        self.0.to_string()
    }
}

impl fmt::Display for ToolCallId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for ToolCallId {
    fn as_ref(&self) -> &str {
        Box::leak(self.0.to_string().into_boxed_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_id_new_v7_is_version_7() {
        let id = ChatId::new_v7();
        assert_eq!(id.as_uuid().get_version_num(), 7);
    }

    #[test]
    fn test_chat_id_parse_uuid7_accepts_v7() {
        let id = ChatId::new_v7();
        let s = id.to_string();
        let parsed = ChatId::parse_uuid7(&s).unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn test_chat_id_parse_uuid7_rejects_v4() {
        // Hardcoded v4 UUID
        let v4 = "550e8400-e29b-41d4-a716-446655440000";
        assert!(ChatId::parse_uuid7(v4).is_err());
    }

    #[test]
    fn test_chat_id_parse_uuid7_rejects_invalid() {
        assert!(ChatId::parse_uuid7("not-a-uuid").is_err());
        assert!(ChatId::parse_uuid7("").is_err());
    }

    #[test]
    fn test_chat_id_from_legacy_or_new_generates_new_for_invalid() {
        let id = ChatId::from_legacy_or_new("chat-1");
        assert_eq!(id.as_uuid().get_version_num(), 7);
    }

    #[test]
    fn test_chat_id_from_legacy_or_new_preserves_v7() {
        let original = ChatId::new_v7();
        let s = original.to_string();
        let restored = ChatId::from_legacy_or_new(&s);
        assert_eq!(restored, original);
    }

    #[test]
    fn test_turn_id_new_v7_is_version_7() {
        let id = ChatTurnId::new_v7();
        assert_eq!(id.as_uuid().get_version_num(), 7);
    }

    #[test]
    fn test_tool_call_id_new_v7_is_version_7() {
        let id = ToolCallId::new_v7();
        assert_eq!(id.as_uuid().get_version_num(), 7);
    }

    #[test]
    fn test_tool_call_id_parse_rejects_tool_1() {
        assert!(ToolCallId::parse_uuid7("tool-1").is_err());
    }
}
