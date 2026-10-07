//! crate 身份常量（#1146 轨道 B 归位：自 `lib.rs` 抽出）。
//!
//! `lib.rs` 仅保留 re-export，公共路径 `share::COMPILED_VERSION` 与 crate 内
//! `crate::LOG_TARGET` 保持不变。

/// 本 crate 的日志 target。
pub(crate) const LOG_TARGET: &str = "aemeath:shared";

/// 编译期注入的版本号，来源于 build.rs 从 git tag 注入的 `AEMEATH_VERSION`；
/// 取不到时 fallback 到 `Cargo.toml` 的 `version`（占位符 `0.0.0`）。
pub const COMPILED_VERSION: &str = match option_env!("AEMEATH_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

/// [`crate::version`] 的运行时缓存：首次调用后进程内返回同一个值。
pub(crate) static VERSION_CACHE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// 前缀 typed id 的统一分隔符（wanaka 方向，#252）。
pub const TYPED_ID_SEPARATOR: &str = "_";

/// base62 有序字符表（#1884：typed id 雪花后缀编码；ASCII 升序保证字典序=数值序）。
pub const BASE62_CHARS: &[u8; 62] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// 雪花 id epoch（2026-01-01T00:00:00Z 毫秒，#1884）。
pub const SNOWFLAKE_EPOCH_MS: u64 = 1_767_225_600_000;
