//! 诊断日志格式化：`&log::Record` → 14 字段紧凑 JSON 行。
//!
//! 字段固定：`ts / boot_ts / ver / session / chat / turn / request_id /
//! model / provider / role / level / target / event_type / msg`。
//! 消费者可用 `grep` / `jq` / `wc -l` 配合其他 `*.log` 文件统一处理（详见 `format_audit_json_line`）。
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
        "event_type": Value::Null,
        "msg": msg,
    });
    serde_json::to_string(&line).unwrap_or_default()
}

/// 把审计日志包装为一行紧凑 JSON：调用方提供的 payload + 全局上下文。
///
/// 输出形态：`{ts, boot_ts, ver, session, chat, turn, request_id, model,
/// provider, role, level, target, event_type, msg}` + payload 字段平铺。
///
/// `event_type` 由调用方传入（`"input" | "output" | "tool_call" | "tool_result"` 等）。
///
/// 调用方提供的 `payload` 字段会与上下文字段**平铺**到同一对象。
pub fn format_audit_json_line(event_type: &str, payload: Value) -> String {
    let mut line = match payload {
        Value::Object(map) => map,
        other => {
            // payload 非对象时退化为 {"payload": other}，避免破坏 JSON 形态
            let mut map = serde_json::Map::new();
            map.insert("payload".to_string(), other);
            map
        }
    };
    line.insert("ts".to_string(), Value::String(timestamp_local_rfc3339()));
    line.insert(
        "boot_ts".to_string(),
        context::boot_ts()
            .map(|s| Value::String(s.to_string()))
            .unwrap_or(Value::Null),
    );
    line.insert(
        "ver".to_string(),
        context::app_version()
            .map(|s| Value::String(s.to_string()))
            .unwrap_or(Value::Null),
    );
    line.insert(
        "session".to_string(),
        Value::String(context::session_id().unwrap_or("-").to_string()),
    );
    line.insert(
        "chat".to_string(),
        Value::String(context::current_chat_id().unwrap_or_else(|| "-".to_string())),
    );
    line.insert("turn".to_string(), turn_to_value(context::current_turn()));
    line.insert(
        "request_id".to_string(),
        context::current_request_id()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    line.insert(
        "model".to_string(),
        Value::String(context::current_model().unwrap_or_else(|| "-".to_string())),
    );
    line.insert(
        "provider".to_string(),
        context::current_provider()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    line.insert(
        "role".to_string(),
        context::current_role()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    line.insert("level".to_string(), Value::String("AUDIT".to_string()));
    line.insert("target".to_string(), Value::String("audit".to_string()));
    line.insert(
        "event_type".to_string(),
        Value::String(event_type.to_string()),
    );
    line.insert("msg".to_string(), Value::Null);
    serde_json::to_string(&Value::Object(line)).unwrap_or_default()
}

fn turn_to_value(turn: Option<usize>) -> Value {
    match turn {
        Some(n) => Value::Number(serde_json::Number::from(n)),
        None => Value::Null,
    }
}

/// 纯文本日志行格式化（用于 stderr 回退模式）。
pub fn format_text_line_with_turn(
    session_id: &str,
    turn: Option<usize>,
    level: &str,
    module: &str,
    message: &str,
) -> String {
    let turn = turn
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());
    format!(
        "[{}] [session:{}] [turn:{}] [{}] [{}] {}",
        timestamp_local_rfc3339(),
        session_id,
        turn,
        level,
        module,
        message
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn diag_line_has_fourteen_fields() {
        let line = format_diag_json_line_from_parts("INFO", "cli::render", "frame");
        let value: Value = serde_json::from_str(&line).expect("valid json");
        let obj = value.as_object().expect("object");
        assert_eq!(obj.len(), 14);
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
            "event_type",
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

    #[test]
    fn audit_line_includes_event_type_and_payload() {
        let payload = json!({ "messages": [{"role": "user"}] });
        let line = format_audit_json_line("input", payload);
        let value: Value = serde_json::from_str(&line).expect("valid json");
        assert_eq!(value["event_type"], "input");
        assert_eq!(value["messages"][0]["role"], "user");
        // 上下文字段存在（值可能为 null / "-"）
        assert!(value.get("ts").is_some());
        assert!(value.get("boot_ts").is_some());
        assert!(value.get("ver").is_some());
        assert!(value.get("session").is_some());
        assert!(value.get("chat").is_some());
        assert!(value.get("turn").is_some());
        assert!(value.get("request_id").is_some());
        assert!(value.get("model").is_some());
        assert!(value.get("provider").is_some());
        assert!(value.get("role").is_some());
        assert!(value.get("level").is_some());
        assert!(value.get("target").is_some());
        assert!(value.get("msg").is_some());
    }

    #[test]
    fn audit_line_merges_payload_fields() {
        let payload = json!({ "stop_reason": "end_turn", "input_tokens": 10 });
        let line = format_audit_json_line("output", payload);
        let value: Value = serde_json::from_str(&line).expect("valid json");
        assert_eq!(value["stop_reason"], "end_turn");
        assert_eq!(value["input_tokens"], 10);
        assert_eq!(value["event_type"], "output");
    }

    #[test]
    fn audit_line_non_object_payload_wrapped() {
        let payload = json!([1, 2, 3]);
        let line = format_audit_json_line("input", payload);
        let value: Value = serde_json::from_str(&line).expect("valid json");
        assert_eq!(value["payload"], json!([1, 2, 3]));
    }

    #[test]
    fn audit_line_is_compact_single_line() {
        let payload = json!({ "messages": [{"role": "user", "content": "hi"}] });
        let line = format_audit_json_line("input", payload);
        assert!(!line.contains('\n'), "audit line must not contain newline");
    }
}
