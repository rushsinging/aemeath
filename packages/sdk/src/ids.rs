//! UUIDv7 newtypes for internal chat, turn, and tool call IDs.
//!
//! Each newtype stores a UUIDv7 plus a pre-formatted string cache. The cache
//! enables zero-allocation `AsRef<str>` and borrowed `as_str()` access,
//! avoiding the previous `Box::leak` pattern that leaked memory on every
//! call. Equality and hashing only consider the UUID, so the cache never
//! affects identity semantics. Serialization is custom to preserve the
//! single-string wire format.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::hash::{Hash, Hasher};
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

/// Build the cached string for a UUID (single source of truth for formatting).
#[inline]
fn cache(uuid: Uuid) -> String {
    uuid.to_string()
}

/// Generates the shared trait impls for a UUIDv7-backed ID newtype whose
/// tuple struct shape is `(Uuid, String)`.
///
/// Equality and hashing only consider the UUID so the cached string never
/// affects identity semantics. `Display` and `AsRef<str>` expose the cached
/// string for zero-allocation borrowed access. SerDe preserves the
/// single-string wire format by serializing only the UUID.
macro_rules! impl_id_type {
    ($ty:ident) => {
        impl PartialEq for $ty {
            fn eq(&self, other: &Self) -> bool {
                self.0 == other.0
            }
        }

        impl Eq for $ty {}

        impl Hash for $ty {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.0.hash(state);
            }
        }

        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.1)
            }
        }

        impl AsRef<str> for $ty {
            fn as_ref(&self) -> &str {
                &self.1
            }
        }

        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
                // Preserve the single-string wire format.
                self.0.serialize(ser)
            }
        }

        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
                let uuid = Uuid::deserialize(de)?;
                if uuid.get_version_num() != 7 {
                    return Err(serde::de::Error::custom(format!(
                        "UUID 不是 version 7: {uuid}"
                    )));
                }
                Ok(Self(uuid, cache(uuid)))
            }
        }
    };
}

macro_rules! define_id_type {
    ($ty:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone)]
        pub struct $ty(Uuid, String);

        impl $ty {
            pub fn new_v7() -> Self {
                let uuid = Uuid::now_v7();
                Self(uuid, cache(uuid))
            }

            pub fn new(s: impl AsRef<str>) -> Self {
                Self::from_legacy_or_new(s.as_ref())
            }

            pub fn parse_uuid7(s: &str) -> Result<Self, IdParseError> {
                let uuid =
                    Uuid::parse_str(s).map_err(|_| IdParseError::InvalidUuid(s.to_string()))?;
                if uuid.get_version_num() != 7 {
                    return Err(IdParseError::NotVersion7(s.to_string()));
                }
                Ok(Self(uuid, cache(uuid)))
            }

            pub fn from_legacy_or_new(s: &str) -> Self {
                Self::parse_uuid7(s).unwrap_or_else(|_| {
                    let uuid = deterministic_uuidv7(s);
                    Self(uuid, cache(uuid))
                })
            }

            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            pub fn as_str(&self) -> &str {
                &self.1
            }
        }

        impl_id_type!($ty);
    };
}

// ---------------------------------------------------------------------------
// ChatId
// ---------------------------------------------------------------------------

/// Internal chat ID (UUIDv7).
#[derive(Debug, Clone)]
pub struct ChatId(Uuid, String);

impl ChatId {
    /// Generate a new UUIDv7 chat ID.
    pub fn new_v7() -> Self {
        let uuid = Uuid::now_v7();
        let s = cache(uuid);
        Self(uuid, s)
    }

    /// Create a ChatId from a legacy string or generate new UUIDv7.
    /// Alias for `from_legacy_or_new` — use `new_v7()` for fresh IDs.
    pub fn new(s: impl AsRef<str>) -> Self {
        Self::from_legacy_or_new(s.as_ref())
    }

    /// Parse a UUIDv7 string as a ChatId.
    pub fn parse_uuid7(s: &str) -> Result<Self, IdParseError> {
        let uuid = Uuid::parse_str(s).map_err(|_| IdParseError::InvalidUuid(s.to_string()))?;
        if uuid.get_version_num() != 7 {
            return Err(IdParseError::NotVersion7(s.to_string()));
        }
        Ok(Self(uuid, cache(uuid)))
    }

    /// Convert from legacy string or generate new UUIDv7.
    /// If the input is a valid UUIDv7, preserves it.
    /// Otherwise, deterministically generates a UUIDv7 from the input string.
    pub fn from_legacy_or_new(s: &str) -> Self {
        Self::parse_uuid7(s).unwrap_or_else(|_| {
            let uuid = deterministic_uuidv7(s);
            Self(uuid, cache(uuid))
        })
    }

    /// Get the inner UUID.
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    /// Get string representation (borrowed from the cached field).
    pub fn as_str(&self) -> &str {
        &self.1
    }
}

impl_id_type!(ChatId);

// ---------------------------------------------------------------------------
// ChatTurnId
// ---------------------------------------------------------------------------

/// Internal turn ID (UUIDv7).
#[derive(Debug, Clone)]
pub struct ChatTurnId(Uuid, String);

impl ChatTurnId {
    /// Generate a new UUIDv7 turn ID.
    pub fn new_v7() -> Self {
        let uuid = Uuid::now_v7();
        Self(uuid, cache(uuid))
    }

    /// Create a ChatTurnId from a legacy string or generate new UUIDv7.
    /// Alias for `from_legacy_or_new` — use `new_v7()` for fresh IDs.
    pub fn new(s: impl AsRef<str>) -> Self {
        Self::from_legacy_or_new(s.as_ref())
    }

    /// Parse a UUIDv7 string as a ChatTurnId.
    pub fn parse_uuid7(s: &str) -> Result<Self, IdParseError> {
        let uuid = Uuid::parse_str(s).map_err(|_| IdParseError::InvalidUuid(s.to_string()))?;
        if uuid.get_version_num() != 7 {
            return Err(IdParseError::NotVersion7(s.to_string()));
        }
        Ok(Self(uuid, cache(uuid)))
    }

    /// Convert from legacy string or generate new UUIDv7.
    /// If the input is a valid UUIDv7, preserves it.
    /// Otherwise, deterministically generates a UUIDv7 from the input string.
    pub fn from_legacy_or_new(s: &str) -> Self {
        Self::parse_uuid7(s).unwrap_or_else(|_| {
            let uuid = deterministic_uuidv7(s);
            Self(uuid, cache(uuid))
        })
    }

    /// Get the inner UUID.
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    /// Get string representation (borrowed from the cached field).
    pub fn as_str(&self) -> &str {
        &self.1
    }
}

impl_id_type!(ChatTurnId);

// ---------------------------------------------------------------------------
// RunId
// ---------------------------------------------------------------------------

/// Published Run identity (UUIDv7).
#[derive(Debug, Clone)]
pub struct RunId(Uuid, String);

impl RunId {
    pub fn new_v7() -> Self {
        let uuid = Uuid::now_v7();
        Self(uuid, cache(uuid))
    }

    pub fn new(s: impl AsRef<str>) -> Self {
        Self::from_legacy_or_new(s.as_ref())
    }

    pub fn parse_uuid7(s: &str) -> Result<Self, IdParseError> {
        let uuid = Uuid::parse_str(s).map_err(|_| IdParseError::InvalidUuid(s.to_string()))?;
        if uuid.get_version_num() != 7 {
            return Err(IdParseError::NotVersion7(s.to_string()));
        }
        Ok(Self(uuid, cache(uuid)))
    }

    pub fn from_legacy_or_new(s: &str) -> Self {
        Self::parse_uuid7(s).unwrap_or_else(|_| {
            let uuid = deterministic_uuidv7(s);
            Self(uuid, cache(uuid))
        })
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    pub fn as_str(&self) -> &str {
        &self.1
    }
}

impl_id_type!(RunId);

define_id_type!(RunStepId, "Published Run Step identity (UUIDv7).");
define_id_type!(
    AgentId,
    "Published Agent identity used for Main/Sub routing (UUIDv7)."
);
define_id_type!(
    InteractionRequestId,
    "Published identity for one Runtime-owned interaction request (UUIDv7)."
);

// ---------------------------------------------------------------------------
// ToolCallId
// ---------------------------------------------------------------------------

/// Internal tool call ID (UUIDv7).
#[derive(Debug, Clone)]
pub struct ToolCallId(Uuid, String);

impl ToolCallId {
    /// Generate a new UUIDv7 tool call ID.
    pub fn new_v7() -> Self {
        let uuid = Uuid::now_v7();
        Self(uuid, cache(uuid))
    }

    /// Create a ToolCallId from a legacy string or generate new UUIDv7.
    /// Alias for `from_legacy_or_new` — use `new_v7()` for fresh IDs.
    pub fn new(s: impl AsRef<str>) -> Self {
        Self::from_legacy_or_new(s.as_ref())
    }

    /// Parse a UUIDv7 string as a ToolCallId.
    pub fn parse_uuid7(s: &str) -> Result<Self, IdParseError> {
        let uuid = Uuid::parse_str(s).map_err(|_| IdParseError::InvalidUuid(s.to_string()))?;
        if uuid.get_version_num() != 7 {
            return Err(IdParseError::NotVersion7(s.to_string()));
        }
        Ok(Self(uuid, cache(uuid)))
    }

    /// Convert from legacy string or generate new UUIDv7.
    /// If the input is a valid UUIDv7, preserves it.
    /// Otherwise, deterministically generates a UUIDv7 from the input string.
    pub fn from_legacy_or_new(s: &str) -> Self {
        Self::parse_uuid7(s).unwrap_or_else(|_| {
            let uuid = deterministic_uuidv7(s);
            Self(uuid, cache(uuid))
        })
    }

    /// Get the inner UUID.
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    /// Get string representation (borrowed from the cached field).
    pub fn as_str(&self) -> &str {
        &self.1
    }
}

impl_id_type!(ToolCallId);

// ---------------------------------------------------------------------------
// InputId
// ---------------------------------------------------------------------------

/// Internal input ID (UUIDv7).
#[derive(Debug, Clone)]
pub struct InputId(Uuid, String);

impl InputId {
    /// Generate a new UUIDv7 input ID.
    pub fn new_v7() -> Self {
        let uuid = Uuid::now_v7();
        Self(uuid, cache(uuid))
    }

    /// Create a InputId from a legacy string or generate new UUIDv7.
    /// Alias for `from_legacy_or_new` — use `new_v7()` for fresh IDs.
    pub fn new(s: impl AsRef<str>) -> Self {
        Self::from_legacy_or_new(s.as_ref())
    }

    /// Parse a UUIDv7 string as a InputId.
    pub fn parse_uuid7(s: &str) -> Result<Self, IdParseError> {
        let uuid = Uuid::parse_str(s).map_err(|_| IdParseError::InvalidUuid(s.to_string()))?;
        if uuid.get_version_num() != 7 {
            return Err(IdParseError::NotVersion7(s.to_string()));
        }
        Ok(Self(uuid, cache(uuid)))
    }

    /// Convert from legacy string or generate new UUIDv7.
    /// If the input is a valid UUIDv7, preserves it.
    /// Otherwise, deterministically generates a UUIDv7 from the input string.
    pub fn from_legacy_or_new(s: &str) -> Self {
        Self::parse_uuid7(s).unwrap_or_else(|_| {
            let uuid = deterministic_uuidv7(s);
            Self(uuid, cache(uuid))
        })
    }

    /// Get the inner UUID.
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    /// Get string representation (borrowed from the cached field).
    pub fn as_str(&self) -> &str {
        &self.1
    }
}

impl_id_type!(InputId);

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_id_new_v7_is_version_7() {
        let id = RunId::new_v7();
        assert_eq!(id.as_uuid().get_version_num(), 7);
        assert_eq!(RunId::parse_uuid7(id.as_str()).unwrap(), id);
    }

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

    // ---- New tests for the cached String field & AsRef<str> ----

    #[test]
    fn test_chat_id_as_str_is_borrowed_no_allocation() {
        let id = ChatId::new_v7();
        let s1: &str = id.as_str();
        let s2: &str = id.as_str();
        // Borrowed from the same backing String — pointer equality.
        assert_eq!(s1.as_ptr(), s2.as_ptr());
        assert_eq!(s1, id.to_string().as_str());
    }

    #[test]
    fn test_chat_id_as_ref_str_returns_cached_value() {
        let id = ChatId::new_v7();
        let expected = id.as_uuid().to_string();
        assert_eq!(id.as_ref(), expected.as_str());
        // And a second call returns the same backing buffer.
        assert_eq!(id.as_ref().as_ptr(), id.as_str().as_ptr());
    }

    #[test]
    fn test_chat_id_from_legacy_or_new_caches_uuid_string() {
        let id = ChatId::from_legacy_or_new("chat-1");
        let uuid_str = id.as_uuid().to_string();
        assert_eq!(id.as_str(), uuid_str);
    }

    #[test]
    fn test_chat_id_equality_ignores_cache_difference() {
        // Two ChatIds built from the same UUID string must compare equal.
        // The cached String buffer may live at a different address but
        // identity only depends on the UUID.
        let a = ChatId::from_legacy_or_new("01900000-0000-7000-8000-000000000000");
        let b = ChatId::from_legacy_or_new("01900000-0000-7000-8000-000000000000");
        assert_ne!(a.as_str().as_ptr(), b.as_str().as_ptr());
        assert_eq!(a, b);
    }

    #[test]
    fn test_chat_id_hash_depends_only_on_uuid() {
        use std::collections::hash_map::DefaultHasher;
        let a = ChatId::new_v7();
        let b = a.clone();
        let mut ha = DefaultHasher::new();
        let mut hb = DefaultHasher::new();
        a.hash(&mut ha);
        b.hash(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    #[test]
    fn test_chat_id_serde_roundtrip_preserves_uuid() {
        let original = ChatId::new_v7();
        let json = serde_json::to_string(&original).unwrap();
        // Wire format must remain a single JSON string.
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_string(), "expected string, got: {parsed}");
        let restored: ChatId = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, original);
    }

    #[test]
    fn test_tool_call_id_serde_roundtrip_preserves_uuid() {
        let original = ToolCallId::new_v7();
        let json = serde_json::to_string(&original).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_string(), "expected string, got: {parsed}");
        let restored: ToolCallId = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, original);
    }

    #[test]
    fn test_chat_turn_id_serde_roundtrip_preserves_uuid() {
        let original = ChatTurnId::new_v7();
        let json = serde_json::to_string(&original).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_string(), "expected string, got: {parsed}");
        let restored: ChatTurnId = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, original);
    }

    #[test]
    fn test_input_id_new_v7_is_version_7() {
        let id = InputId::new_v7();
        assert_eq!(id.as_uuid().get_version_num(), 7);
    }

    #[test]
    fn test_input_id_serde_roundtrip_preserves_uuid() {
        let original = InputId::new_v7();
        let json = serde_json::to_string(&original).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_string(), "expected string, got: {parsed}");
        let restored: InputId = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, original);
    }
}
