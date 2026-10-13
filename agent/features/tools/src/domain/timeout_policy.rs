//! 统一工具 `timeout` 入参解析与 schema 注入。

use serde_json::{json, Value};

use super::constants::DEFAULT_TOOL_MAX_TIMEOUT_SECS;

/// 解析工具 `timeout` 入参（秒）。
///
/// - `None` → `default_secs`
/// - `Some(v)` → clamp 到 `[1, max_secs]`（含 `0` 与超大值）
pub fn resolve_tool_timeout(requested: Option<u64>, default_secs: u64, max_secs: u64) -> u64 {
    let max_secs = max_secs.max(1);
    let default_secs = default_secs.clamp(1, max_secs);
    match requested {
        None => default_secs,
        Some(value) => value.clamp(1, max_secs),
    }
}

/// 从工具入参 JSON 提取可选 `timeout`（秒）。非整数或缺省返回 `None`。
pub fn extract_timeout_from_input(input: &Value) -> Option<u64> {
    input.get("timeout").and_then(Value::as_u64)
}

/// 幂等注入可选 `timeout` 字段到 input JSON Schema。
///
/// 已有 `timeout` 时只对齐 description / bounds，不重复添加；
/// **NEVER** 把 `timeout` 放进 `required`。
pub fn inject_timeout_into_input_schema(
    mut schema: Value,
    default_secs: u64,
    max_secs: u64,
) -> Value {
    let max_secs = max_secs.max(1);
    let default_secs = default_secs.clamp(1, max_secs);
    let description = format!("Timeout in seconds (default {default_secs}, max {max_secs})");
    let timeout_schema = json!({
        "type": "integer",
        "description": description,
        "minimum": 1,
        "maximum": max_secs,
    });

    let Some(object) = schema.as_object_mut() else {
        return json!({
            "type": "object",
            "properties": {
                "timeout": timeout_schema,
            },
            "required": []
        });
    };

    if object.get("type").and_then(Value::as_str) != Some("object") {
        object.insert("type".to_string(), json!("object"));
    }

    let properties = object.entry("properties").or_insert_with(|| json!({}));
    if let Some(properties_object) = properties.as_object_mut() {
        properties_object.insert("timeout".to_string(), timeout_schema);
    }

    if let Some(required) = object.get_mut("required").and_then(Value::as_array_mut) {
        required.retain(|item| item.as_str() != Some("timeout"));
    }

    schema
}

/// ToolDescriptor / serde 缺省 max（通用档）。
pub fn default_tool_max_timeout_secs() -> u64 {
    DEFAULT_TOOL_MAX_TIMEOUT_SECS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_none_uses_default_clamped_to_max() {
        assert_eq!(resolve_tool_timeout(None, 120, 3600), 120);
        assert_eq!(resolve_tool_timeout(None, 5000, 300), 300);
    }

    #[test]
    fn resolve_clamps_zero_and_over_max_to_bounds() {
        assert_eq!(resolve_tool_timeout(Some(0), 120, 3600), 1);
        assert_eq!(resolve_tool_timeout(Some(99999), 120, 3600), 3600);
        assert_eq!(resolve_tool_timeout(Some(50), 120, 3600), 50);
    }

    #[test]
    fn extract_reads_integer_timeout_only() {
        assert_eq!(extract_timeout_from_input(&json!({})), None);
        assert_eq!(
            extract_timeout_from_input(&json!({"timeout": 12})),
            Some(12)
        );
        assert_eq!(extract_timeout_from_input(&json!({"timeout": "12"})), None);
    }

    #[test]
    fn inject_adds_timeout_and_is_idempotent() {
        let schema = json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        });
        let once = inject_timeout_into_input_schema(schema, 120, 3600);
        let twice = inject_timeout_into_input_schema(once.clone(), 30, 300);
        let timeout = &twice["properties"]["timeout"];
        assert_eq!(timeout["type"], "integer");
        assert_eq!(timeout["minimum"], 1);
        assert_eq!(timeout["maximum"], 300);
        assert_eq!(
            timeout["description"],
            "Timeout in seconds (default 30, max 300)"
        );
        assert_eq!(twice["required"], json!(["path"]));
        assert!(once["properties"].get("timeout").is_some());
    }

    #[test]
    fn inject_aligns_existing_timeout_description() {
        let schema = json!({
            "type": "object",
            "properties": {
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in seconds (default 3600, max 10800). Use 0 for no timeout."
                }
            },
            "required": ["timeout"]
        });
        let injected = inject_timeout_into_input_schema(schema, 3600, 10800);
        assert_eq!(
            injected["properties"]["timeout"]["description"],
            "Timeout in seconds (default 3600, max 10800)"
        );
        assert_eq!(injected["required"], json!([]));
    }
}
