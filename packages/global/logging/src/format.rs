//! 日志格式化：`&log::Record` → 13 字段紧凑 JSON 行。
//!
//! 字段固定：`ts / boot_ts / ver / session / chat / turn / request_id /
//! model / provider / role / level / target / msg`。
//!
//! 写入格式为 **compact JSON Lines**（一行一个 JSON 对象，无 pretty-print 缩进）。

use crate::context;
use crate::rotation::timestamp_rfc3339;
use log::Record;
use serde_json::{json, Value};

/// 本地时间 RFC3339 格式（含时区偏移），毫秒精度。
///
/// 委托 `rotation::timestamp_rfc3339`（已使用 `chrono::Local`）。
pub fn timestamp_local_rfc3339() -> String {
    timestamp_rfc3339()
}

/// 把诊断日志 `Record` 序列化为一行紧凑 JSON。
///
/// `turn` 字段在未设置时为 `null`（其他字段若无值用 `"-"` 占位）。
pub fn format_diag_json_line(record: &Record) -> String {
    format_diag_json_line_from_parts(
        record.level().as_str(),
        record.target(),
        &record.args().to_string(),
    )
}

/// 内部 helper：给定诊断日志的三个核心字段（level/target/msg），序列化为一行紧凑 JSON。
///
/// 拆分目的是让测试不必构造 `log::Record`（`Record::builder().args(format_args!(...))`
/// 会产生借用临时值的错误）。
pub fn format_diag_json_line_from_parts(level: &str, target: &str, msg: &str) -> String {
    let line = json!({
        "ts": timestamp_local_rfc3339(),
        "boot_ts": context::boot_ts(),
        "ver": context::app_version(),
        "session": Value::String(context::session_id().unwrap_or("-").to_string()),
        "chat": Value::String(context::current_chat_id().as_deref().unwrap_or("-").to_string()),
        "turn": context::current_turn(),
        "request_id": context::current_request_id(),
        "model": Value::String(context::current_model().as_deref().unwrap_or("-").to_string()),
        "provider": context::current_provider(),
        "role": context::current_role(),
        "level": level,
        "target": target,
        "msg": msg,
    });
    serde_json::to_string(&line).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn diag_line_has_thirteen_fields() {
        let line = format_diag_json_line_from_parts("INFO", "cli::render", "frame");
        let value: Value = serde_json::from_str(&line).expect("valid json");
        let obj = value.as_object().expect("object");
        assert_eq!(obj.len(), 13);
        for key in [
            "ts",
            "boot_ts",
            "ver",
            "session",
            "chat",
            "turn",
            "request_id",
            "model",
            "provider",
            "role",
            "level",
            "target",
            "msg",
        ] {
            assert!(obj.contains_key(key), "missing key: {key}");
        }
        assert_eq!(obj["level"], "INFO");
        assert_eq!(obj["target"], "cli::render");
        assert_eq!(obj["msg"], "frame");
    }

    #[test]
    fn diag_line_is_compact_single_line() {
        let line = format_diag_json_line_from_parts("INFO", "test", "hello");
        assert!(!line.contains('\n'), "diag line must not contain newline");
    }
}
