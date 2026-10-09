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
    #[error("无效的前缀 typed id 形态: {0}")]
    InvalidPrefixedFormat(String),
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

/// 生成前缀形态 typed id：`<prefix>_<11 字符 base62 雪花>`。
///
/// 64 bit 雪花（41 时间 + 10 进程随机 + 12 序列）经有序 base62 定长
/// 编码——同前缀下**字典序 = 时间序**。前缀词汇表逐步接入（先 `task`，
/// 其余 id 渐进迁移）。
pub fn new_typed_id(prefix: &str) -> String {
    format!(
        "{prefix}{}{}",
        crate::constants::TYPED_ID_SEPARATOR,
        encode_base62_fixed(generate_snowflake())
    )
}

/// 校验前缀形态 typed id：`<prefix>_<11 位 base62>`（全锚定形状）。
pub fn is_typed_id(value: &str, prefix: &str) -> bool {
    let Some(rest) = value.strip_prefix(prefix) else {
        return false;
    };
    let Some(suffix) = rest.strip_prefix(crate::constants::TYPED_ID_SEPARATOR) else {
        return false;
    };
    suffix.len() == 11
        && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
        && decode_base62(suffix).is_some()
}

// ── 雪花 id（#1884）：41 时间 + 10 进程随机 + 12 序列 ────────────────

/// 生成 64 bit 雪花 id（线程安全；时钟回拨时借用上次毫秒继续序列）。
///
/// 生成状态（函数内 static，使用点内聚）：
/// - `process_id`：进程随机 10 bit（防多 CLI 实例跨进程碰撞）；
/// - `state`：高 52 bit 上次毫秒（epoch 相对）+ 低 12 bit 序列，CAS 推进。
pub fn generate_snowflake() -> u64 {
    static SNOWFLAKE_PROCESS_ID: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
    static SNOWFLAKE_STATE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let process_id = *SNOWFLAKE_PROCESS_ID.get_or_init(|| {
        // 进程随机 10 bit（uuid 高位熵源足够）。
        (Uuid::now_v7().as_u64_pair().0 & 0x3FF) as u16
    });
    loop {
        let state = SNOWFLAKE_STATE.load(std::sync::atomic::Ordering::Relaxed);
        let (last_ms, seq) = (state >> 12, state & 0xFFF);
        let now_ms = current_millis_since_epoch();
        let (ms, next_seq) = if now_ms > last_ms {
            (now_ms, 0)
        } else {
            // 同毫秒或时钟回拨：沿用上次毫秒，序列递增。
            (last_ms, seq + 1)
        };
        if next_seq > 0xFFF {
            // 序列耗尽：等待下一毫秒（不阻塞.spin_yield 让出）。
            std::thread::yield_now();
            continue;
        }
        let next_state = (ms << 12) | next_seq;
        if SNOWFLAKE_STATE
            .compare_exchange(
                state,
                next_state,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Relaxed,
            )
            .is_ok()
        {
            return ((ms - crate::constants::SNOWFLAKE_EPOCH_MS) << 22)
                | ((process_id as u64) << 12)
                | next_seq;
        }
    }
}

/// 当前 unix 毫秒（经 uuid v7 时间戳提取——share minimal-kernel 禁
/// 直连 `SystemTime::now`，uuid crate 的时间源是既有内核依赖）。
fn current_millis_since_epoch() -> u64 {
    // uuid v7 布局：前 48 bit（大端）= unix 毫秒时间戳；
    // `as_u64_pair().0` 是前 8 字节 → 时间戳 = 高 48 bit = `>> 16`。
    Uuid::now_v7().as_u64_pair().0 >> 16
}

/// 64 bit → base62 定长 11 字符（前导补零；有序 alphabet）。
pub fn encode_base62_fixed(value: u64) -> String {
    let alphabet = crate::constants::BASE62_CHARS;
    let mut digits = [0u8; 11];
    let mut rest = value;
    for slot in digits.iter_mut().rev() {
        *slot = alphabet[(rest % 62) as usize];
        rest /= 62;
    }
    String::from_utf8(digits.to_vec()).expect("base62 字符表必然是合法 UTF-8")
}

/// base62 字符串 → 64 bit（非法字符返回 None）。
pub fn decode_base62(value: &str) -> Option<u64> {
    if value.is_empty() || value.len() > 11 {
        return None;
    }
    let mut result: u64 = 0;
    for byte in value.bytes() {
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'A'..=b'Z' => byte - b'A' + 10,
            b'a'..=b'z' => byte - b'a' + 36,
            _ => return None,
        } as u64;
        result = result.checked_mul(62)?.checked_add(digit)?;
    }
    Some(result)
}

/// Runtime-owned 后台进程记录标识（前缀 typed id，`bgp_<base62>`）。
///
/// 本体即前缀形态（单一真相，无裸值/display 两套）；
/// `parse` 按前缀+形状校验，跨种类误用在前缀层被拒。
/// 历史快照保留解析兼容：`process_`（短前缀修订前）与 `task_`
/// （Background Process 更名前）；读旧写新，新生成一律 `bgp_`。
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct BackgroundProcessId(String);

impl BackgroundProcessId {
    /// 生成新进程 id（`bgp_` + 雪花 base62；字典序=时间序）。
    pub fn new_v7() -> Self {
        Self(new_typed_id("bgp"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 解析（严格前缀+形状校验；兼容 `process_` / `task_` 旧快照）。
    pub fn parse(value: &str) -> Result<Self, IdParseError> {
        if !(is_typed_id(value, "bgp")
            || is_typed_id(value, "process")
            || is_typed_id(value, "task"))
        {
            return Err(IdParseError::InvalidPrefixedFormat(value.to_string()));
        }
        Ok(Self(value.to_string()))
    }
}

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
