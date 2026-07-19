//! 日志格式化：`&log::Record` → 14 字段紧凑 JSON 行。
//!
//! 字段固定：`ts / boot_ts / pid / ver / session / chat / turn / request_id /
//! model / provider / role / level / target / msg`。
//!
//! 写入格式为 **compact JSON Lines**（一行一个 JSON 对象，无 pretty-print 缩进）。

use super::context as log_context;
use super::lifecycle::timestamp_rfc3339;
use crate::domain::LogContext;
use log::Record;
use serde_json::{json, Value};

/// 本地时间 RFC3339 格式（含时区偏移），毫秒精度。
///
/// 委托 `lifecycle::timestamp_rfc3339`（已使用 `chrono::Local`）。
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
    let resolved = log_context::scoped_context().unwrap_or_default();
    format_diag_json_line_with_context(level, target, msg, &resolved)
}

/// 使用调用方显式提供的不可变 context 格式化，供同步线程安全传播。
pub(crate) fn format_diag_json_line_with_context(
    level: &str,
    target: &str,
    msg: &str,
    context_snapshot: &LogContext,
) -> String {
    let line = json!({
        "ts": timestamp_local_rfc3339(),
        "boot_ts": log_context::boot_ts(),
        "pid": log_context::pid(),
        "ver": log_context::app_version(),
        "session": Value::String(context_snapshot.session_id.as_deref().unwrap_or("-").to_string()),
        "chat": Value::String(context_snapshot.chat_id.as_deref().unwrap_or("-").to_string()),
        "turn": context_snapshot.turn,
        "request_id": context_snapshot.request_id,
        "model": Value::String(context_snapshot.model.as_deref().unwrap_or("-").to_string()),
        "provider": context_snapshot.provider,
        "role": context_snapshot.role,
        "level": level,
        "target": target,
        "msg": msg,
    });
    serde_json::to_string(&line).unwrap_or_default()
}

#[cfg(test)]
#[path = "formatter_tests.rs"]
mod tests;
