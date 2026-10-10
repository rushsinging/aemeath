//! Prefixed typed IDs（雪花 base62）与 legacy UUIDv7 读兼容。
//!
//! 新生成一律 `prefix_<11 base62>`（`new_typed_id`）；parse / serde 读路径
//! 同时接受旧 UUIDv7 字符串。`from_legacy_or_new` 对非法串做确定性 UUIDv7 映射（ACL/测试稳定）；
//! 新鲜生成仍走 `new_v7` → typed id。

use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use uuid::Uuid;

/// Error returned when parsing an internal ID string.
#[derive(Debug, Clone, thiserror::Error)]
pub enum IdParseError {
    #[error("无效的 UUID 格式: {0}")]
    InvalidUuid(String),
    #[error("UUID 不是 version 7: {0}")]
    NotVersion7(String),
    #[error("无效的前缀 typed id 形态: {0}")]
    InvalidPrefixedFormat(String),
}

/// 是否为合法 legacy UUIDv7 字符串。
fn is_legacy_uuidv7(value: &str) -> bool {
    match Uuid::parse_str(value) {
        Ok(uuid) => uuid.get_version_num() == 7,
        Err(_) => false,
    }
}

/// Deterministic UUIDv7 from a string（namespace-based；仅 `from_legacy_or_new` 非法串路径）。
fn deterministic_uuidv7(s: &str) -> Uuid {
    let namespace = Uuid::from_bytes([
        0xa1, 0x7e, 0x0a, 0x7e, 0x0a, 0x7e, 0x0a, 0x7e, 0xa1, 0x7e, 0x0a, 0x7e, 0x0a, 0x7e, 0x0a,
        0x7e,
    ]);
    let base = Uuid::new_v5(&namespace, s.as_bytes());
    let mut bytes = *base.as_bytes();
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

macro_rules! define_typed_id {
    ($ty:ident, $prefix:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $ty(String);

        impl $ty {
            /// 此前缀登记名（specs 3.2.1.2）。
            pub const PREFIX: &'static str = $prefix;

            /// 生成新 typed id（`prefix_<11 base62>`）。
            pub fn new_v7() -> Self {
                Self(new_typed_id($prefix))
            }

            /// 从字符串构造：合法 typed / uuidv7 保留，否则确定性 UUIDv7。
            pub fn new(s: impl AsRef<str>) -> Self {
                Self::from_legacy_or_new(s.as_ref())
            }

            /// 严格解析：typed（本前缀）或 legacy UUIDv7。
            pub fn parse(s: &str) -> Result<Self, IdParseError> {
                if is_typed_id(s, $prefix) || is_legacy_uuidv7(s) {
                    Ok(Self(s.to_string()))
                } else if Uuid::parse_str(s).is_ok() {
                    Err(IdParseError::NotVersion7(s.to_string()))
                } else if s.contains(crate::constants::TYPED_ID_SEPARATOR) {
                    Err(IdParseError::InvalidPrefixedFormat(s.to_string()))
                } else {
                    Err(IdParseError::InvalidUuid(s.to_string()))
                }
            }

            /// 兼容旧名；等价于 [`Self::parse`]。
            pub fn parse_uuid7(s: &str) -> Result<Self, IdParseError> {
                Self::parse(s)
            }

            /// 读旧写新辅助：可 parse 则保留，否则确定性 UUIDv7（稳定同输入）。
            pub fn from_legacy_or_new(s: &str) -> Self {
                Self::parse(s).unwrap_or_else(|_| Self(deterministic_uuidv7(s).to_string()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl AsRef<str> for $ty {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
                ser.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
                let value = String::deserialize(de)?;
                Self::parse(&value).map_err(serde::de::Error::custom)
            }
        }

        impl JsonSchema for $ty {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($ty).into()
            }

            fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
                json_schema!({
                    "type": "string",
                    "description": concat!(
                        "Typed id `",
                        $prefix,
                        "_<11 base62>` or legacy UUIDv7"
                    )
                })
            }
        }
    };
}

define_typed_id!(
    ChatId,
    "cht",
    "Internal chat / session correlation id（typed `cht_`；兼容 legacy UUIDv7）。"
);
define_typed_id!(
    ChatRunId,
    "run",
    "TUI/runtime chat-run correlation id（typed `run_`，与 RunId 同前缀）。"
);
define_typed_id!(
    RunId,
    "run",
    "Published Run identity（typed `run_`；兼容 legacy UUIDv7）。"
);
define_typed_id!(
    SessionId,
    "ses",
    "Context-owned Session identity（typed `ses_`；兼容 legacy UUIDv7）。"
);
define_typed_id!(
    RunStepId,
    "stp",
    "Published Run Step identity（typed `stp_`；兼容 legacy UUIDv7）。"
);
define_typed_id!(
    ModelInvocationId,
    "inv",
    "Runtime-owned model invocation identity（typed `inv_`；兼容 legacy UUIDv7）。"
);
define_typed_id!(
    AgentId,
    "agt",
    "Published Agent identity（typed `agt_`；兼容 legacy UUIDv7）。"
);
define_typed_id!(
    InteractionRequestId,
    "irq",
    "Runtime-owned interaction request identity（typed `irq_`；兼容 legacy UUIDv7）。"
);
define_typed_id!(
    ToolCallId,
    "tcl",
    "Internal tool call identity（typed `tcl_`；兼容 legacy UUIDv7）。"
);
define_typed_id!(
    InputId,
    "inp",
    "Internal input identity（typed `inp_`；兼容 legacy UUIDv7）。"
);
define_typed_id!(
    ActivityId,
    "act",
    "Runtime-owned Activity identity（typed `act_`；兼容 legacy UUIDv7）。"
);

/// 生成前缀形态 typed id：`<prefix>_<11 字符 base62 雪花>`。
///
/// 64 bit 雪花（41 时间 + 10 进程随机 + 12 序列）经有序 base62 定长
/// 编码——同前缀下**字典序 = 时间序**。
/// 约定（specs 3.2.1.2）：前缀长度固定 3 字符（`bgp` / `run` 等）；
/// 新前缀 NEVER 超过 3 字符，历史超长前缀仅存在于 parse 兼容读路径。
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
pub fn generate_snowflake() -> u64 {
    static SNOWFLAKE_PROCESS_ID: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
    static SNOWFLAKE_STATE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let process_id =
        *SNOWFLAKE_PROCESS_ID.get_or_init(|| (Uuid::now_v7().as_u64_pair().0 & 0x3FF) as u16);
    loop {
        let state = SNOWFLAKE_STATE.load(std::sync::atomic::Ordering::Relaxed);
        let (last_ms, seq) = (state >> 12, state & 0xFFF);
        let now_ms = current_millis_since_epoch();
        let (ms, next_seq) = if now_ms > last_ms {
            (now_ms, 0)
        } else {
            (last_ms, seq + 1)
        };
        if next_seq > 0xFFF {
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
/// 直连 `SystemTime::now`）。
fn current_millis_since_epoch() -> u64 {
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

impl fmt::Display for BackgroundProcessId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for BackgroundProcessId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
