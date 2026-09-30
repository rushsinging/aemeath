//! UUIDv7 newtypes for internal chat, run, and tool call IDs.
//!
//! Each newtype stores a UUIDv7 plus a pre-formatted string cache. The cache
//! enables zero-allocation `AsRef<str>` and borrowed `as_str()` access,
//! avoiding the previous `Box::leak` pattern that leaked memory on every
//! call. Equality and hashing only consider the UUID, so the cache never
//! affects identity semantics. Serialization is custom to preserve the
//! single-string wire format.

use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
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
        impl JsonSchema for $ty {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($ty).into()
            }

            fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
                json_schema!({
                    "type": "string",
                    "format": "uuid",
                    "x-aemeath-uuid-version": 7
                })
            }
        }

        impl PartialEq for $ty {
            fn eq(&self, other: &Self) -> bool {
                self.0 == other.0
            }
        }

        impl Eq for $ty {}

        /// UUIDv7 的字节序即时间序，跨队列按 id 排序等价于按生成顺序排序
        /// （排队回显需要把消息与命令还原成提交顺序，#1816）。
        impl Ord for $ty {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                self.0.cmp(&other.0)
            }
        }

        impl PartialOrd for $ty {
            fn partial_cmp(
                &self,
                other: &Self,
            ) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

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
// ChatRunId
// ---------------------------------------------------------------------------

/// Internal run ID (UUIDv7).
#[derive(Debug, Clone)]
pub struct ChatRunId(Uuid, String);

impl ChatRunId {
    /// Generate a new UUIDv7 run ID.
    pub fn new_v7() -> Self {
        let uuid = Uuid::now_v7();
        Self(uuid, cache(uuid))
    }

    /// Create a ChatRunId from a legacy string or generate new UUIDv7.
    /// Alias for `from_legacy_or_new` — use `new_v7()` for fresh IDs.
    pub fn new(s: impl AsRef<str>) -> Self {
        Self::from_legacy_or_new(s.as_ref())
    }

    /// Parse a UUIDv7 string as a ChatRunId.
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

impl_id_type!(ChatRunId);

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

define_id_type!(
    SessionId,
    "Context-owned Session identity published for cross-BC correlation (UUIDv7)."
);
define_id_type!(RunStepId, "Published Run Step identity (UUIDv7).");
define_id_type!(
    ModelInvocationId,
    "Runtime-owned identity for one model invocation (UUIDv7)."
);
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
