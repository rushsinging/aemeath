//! 共享时间戳生成（#1863：自 context session 域迁入——无 session 语义）。

use chrono::Utc;

/// 当前时刻的 ISO-8601 / RFC-3339 字符串（秒精度、Z 后缀）。
pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
