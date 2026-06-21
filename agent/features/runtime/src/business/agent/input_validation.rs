//! Tool input schema 预校验（issue #430）。
//!
//! runtime 派发 `tool_use.input` 给工具 `call()` 前，先调用
//! [`validate_tool_input`] 比对 schema，一次性收集全部错误
//! （缺失 required / 多余字段 / 类型不匹配 / enum 违例），返回结构化中文
//! 错误消息，让模型一回合即可看出参数问题并纠正。
//!
//! 设计要点：
//! - **收集全部错误**而非 fail-fast，避免模型逐个重试。
//! - 宽松 schema 自动降级：无 `properties` 整体跳过；`additionalProperties: true`
//!   仅跳过多余字段检查（required / type / enum 仍校验）。
//! - 字段 schema 无 `type`（如 `Value` 字段生成的 `{}`）不校验类型。
//! - `nullable: true` 字段允许传 null。

use serde_json::Value;

/// 类型不匹配记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeMismatch {
    pub field: String,
    pub expected: String,
    pub actual: String,
}

/// 枚举违例记录。
#[derive(Debug, Clone, PartialEq)]
pub struct EnumViolation {
    pub field: String,
    pub allowed: Vec<Value>,
    pub actual: Value,
}

/// 工具输入预校验失败时收集的全部错误。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolInputMismatch {
    pub tool_name: String,
    pub expected: Vec<String>,
    pub actual: Vec<String>,
    pub missing_required: Vec<String>,
    pub unexpected: Vec<String>,
    pub type_mismatch: Vec<TypeMismatch>,
    pub enum_violation: Vec<EnumViolation>,
}

impl ToolInputMismatch {
    pub fn is_empty(&self) -> bool {
        self.missing_required.is_empty()
            && self.unexpected.is_empty()
            && self.type_mismatch.is_empty()
            && self.enum_violation.is_empty()
    }
}

pub fn validate_tool_input(
    tool_name: &str,
    schema: &Value,
    input: &Value,
) -> Result<(), Box<ToolInputMismatch>> {
    // 无 properties 的宽松 schema（如 `{"type":"object"}`）整体跳过。
    let properties = match schema.get("properties").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => return Ok(()),
    };

    // additionalProperties == true 时仅跳过多余字段检查（required/type/enum 仍校验）。
    let allow_additional = schema
        .get("additionalProperties")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let required: Vec<String> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let expected: Vec<String> = properties.keys().cloned().collect();

    let mut mismatch = ToolInputMismatch {
        tool_name: tool_name.to_string(),
        expected,
        actual: Vec::new(),
        missing_required: Vec::new(),
        unexpected: Vec::new(),
        type_mismatch: Vec::new(),
        enum_violation: Vec::new(),
    };

    let input_obj = match input.as_object() {
        Some(o) => o,
        // input 非 object：视为缺少全部 required 字段。
        None => {
            mismatch.actual = Vec::new();
            mismatch.missing_required = required;
            return Err(Box::new(mismatch));
        }
    };

    mismatch.actual = input_obj.keys().cloned().collect();

    // 缺失 required。
    for req in &required {
        if !input_obj.contains_key(req) {
            mismatch.missing_required.push(req.clone());
        }
    }

    // 遍历实际输入字段。
    for (key, value) in input_obj {
        match properties.get(key) {
            None => {
                if !allow_additional {
                    mismatch.unexpected.push(key.clone());
                }
            }
            Some(field_schema) => {
                // 类型检查；类型不符时不重复报 enum（类型错是更根本问题）。
                match check_field_type(field_schema, value) {
                    None => {
                        if let Some(enum_arr) = field_schema.get("enum").and_then(|e| e.as_array())
                        {
                            if !enum_arr.iter().any(|allowed| allowed == value) {
                                mismatch.enum_violation.push(EnumViolation {
                                    field: key.clone(),
                                    allowed: enum_arr.clone(),
                                    actual: value.clone(),
                                });
                            }
                        }
                    }
                    Some((expected_type, actual_type)) => {
                        mismatch.type_mismatch.push(TypeMismatch {
                            field: key.clone(),
                            expected: expected_type,
                            actual: actual_type,
                        });
                    }
                }
            }
        }
    }

    if mismatch.is_empty() {
        Ok(())
    } else {
        Err(Box::new(mismatch))
    }
}

pub fn format_tool_input_error(mismatch: &ToolInputMismatch) -> String {
    let mut lines = Vec::new();
    lines.push(format!("工具「{}」输入参数校验失败。", mismatch.tool_name));
    if !mismatch.expected.is_empty() {
        lines.push(format!("期望字段：{}", mismatch.expected.join(", ")));
    }
    if !mismatch.actual.is_empty() {
        lines.push(format!("实际传入：{}", mismatch.actual.join(", ")));
    }
    if !mismatch.missing_required.is_empty() {
        lines.push(format!(
            "缺失必需字段：{}",
            mismatch.missing_required.join(", ")
        ));
    }
    if !mismatch.unexpected.is_empty() {
        lines.push(format!(
            "多余字段（不在 schema 中，疑似生成噪声）：{}",
            mismatch.unexpected.join(", ")
        ));
    }
    if !mismatch.type_mismatch.is_empty() {
        lines.push("类型不匹配：".to_string());
        for tm in &mismatch.type_mismatch {
            lines.push(format!(
                "  - {}：期望 {}，实际 {}",
                tm.field, tm.expected, tm.actual
            ));
        }
    }
    if !mismatch.enum_violation.is_empty() {
        lines.push("枚举值不合法：".to_string());
        for ev in &mismatch.enum_violation {
            let allowed_str = ev
                .allowed
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!(
                "  - {}：合法值 [{}]，实际 {}",
                ev.field, allowed_str, ev.actual
            ));
        }
    }
    lines.push("请仅传入 schema 定义的字段且类型正确后重试。".to_string());
    lines.join("\n")
}

/// 检查字段值类型是否符合 schema。返回 `Some((expected, actual))` 表示不符。
///
/// 跳过条件：
/// - 字段 schema 无 `type`（如 `Value` 生成的 `{}`）→ `None`（不校验）。
/// - `nullable: true` 且值为 null → `None`。
fn check_field_type(field_schema: &Value, value: &Value) -> Option<(String, String)> {
    let nullable = field_schema
        .get("nullable")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if value.is_null() && nullable {
        return None;
    }
    let expected = field_schema.get("type")?.as_str()?;
    let actual = json_type_of(value);
    let matches = match expected {
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        _ => true, // 未知 type 不校验。
    };
    if matches {
        None
    } else {
        Some((expected.to_string(), actual))
    }
}

/// 返回 serde_json::Value 的 JSON 类型描述（区分 integer 与 number）。
fn json_type_of(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(_) => "boolean".to_string(),
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer".to_string(),
        Value::Number(_) => "number".to_string(),
        Value::String(_) => "string".to_string(),
        Value::Array(_) => "array".to_string(),
        Value::Object(_) => "object".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_schema() -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string", "description": "..." },
                "content": { "type": "string", "description": "..." }
            },
            "required": ["file_path", "content"]
        })
    }

    #[test]
    fn test_validate_tool_input_passes_valid_input() {
        let input = serde_json::json!({ "file_path": "/tmp/a.txt", "content": "hello" });
        assert!(validate_tool_input("Write", &write_schema(), &input).is_ok());
    }

    #[test]
    fn test_validate_tool_input_passes_with_optional_field_omitted() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "a": { "type": "string" },
                "b": { "type": "string", "nullable": true }
            },
            "required": ["a"]
        });
        let input = serde_json::json!({ "a": "x" });
        assert!(validate_tool_input("T", &schema, &input).is_ok());
    }

    #[test]
    fn test_validate_tool_input_collects_missing_required() {
        let input = serde_json::json!({ "content": "hi" });
        let err = validate_tool_input("Write", &write_schema(), &input).unwrap_err();
        assert_eq!(err.missing_required, vec!["file_path".to_string()]);
        assert!(err.unexpected.is_empty());
        assert!(err.type_mismatch.is_empty());
        assert!(err.enum_violation.is_empty());
    }

    #[test]
    fn test_validate_tool_input_collects_all_missing_required() {
        let input = serde_json::json!({});
        let err = validate_tool_input("Write", &write_schema(), &input).unwrap_err();
        assert_eq!(
            err.missing_required,
            vec!["file_path".to_string(), "content".to_string()]
        );
    }

    #[test]
    fn test_validate_tool_input_collects_unexpected_fields() {
        let input = serde_json::json!({
            "Cow": "Borrowed(self.description())",
            "TypedTool": "...",
            "content": "hi",
            "lang": "en"
        });
        let err = validate_tool_input("Write", &write_schema(), &input).unwrap_err();
        assert_eq!(err.missing_required, vec!["file_path".to_string()]);
        let mut unexpected = err.unexpected.clone();
        unexpected.sort();
        assert_eq!(
            unexpected,
            vec![
                "Cow".to_string(),
                "TypedTool".to_string(),
                "lang".to_string()
            ]
        );
    }

    #[test]
    fn test_validate_tool_input_collects_type_mismatch_string_vs_number() {
        let input = serde_json::json!({ "file_path": 123, "content": "hi" });
        let err = validate_tool_input("Write", &write_schema(), &input).unwrap_err();
        assert_eq!(err.type_mismatch.len(), 1);
        assert_eq!(
            err.type_mismatch[0],
            TypeMismatch {
                field: "file_path".to_string(),
                expected: "string".to_string(),
                actual: "integer".to_string(),
            }
        );
    }

    #[test]
    fn test_validate_tool_input_integer_rejects_float() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "n": { "type": "integer" } },
            "required": ["n"]
        });
        let input = serde_json::json!({ "n": 1.5 });
        let err = validate_tool_input("T", &schema, &input).unwrap_err();
        assert_eq!(
            err.type_mismatch,
            vec![TypeMismatch {
                field: "n".to_string(),
                expected: "integer".to_string(),
                actual: "number".to_string(),
            }]
        );
    }

    #[test]
    fn test_validate_tool_input_number_accepts_integer() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "x": { "type": "number" } },
            "required": ["x"]
        });
        let input = serde_json::json!({ "x": 5 });
        assert!(validate_tool_input("T", &schema, &input).is_ok());
    }

    #[test]
    fn test_validate_tool_input_collects_multiple_type_mismatches() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "a": { "type": "string" },
                "b": { "type": "boolean" },
                "c": { "type": "array" }
            },
            "required": ["a", "b", "c"]
        });
        let input = serde_json::json!({ "a": 1, "b": "true", "c": {} });
        let err = validate_tool_input("T", &schema, &input).unwrap_err();
        assert_eq!(err.type_mismatch.len(), 3);
    }

    #[test]
    fn test_validate_tool_input_collects_enum_violation() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "mode": { "type": "string", "enum": ["auto", "manual"] }
            },
            "required": ["mode"]
        });
        let input = serde_json::json!({ "mode": "unknown" });
        let err = validate_tool_input("T", &schema, &input).unwrap_err();
        assert_eq!(err.enum_violation.len(), 1);
        assert_eq!(err.enum_violation[0].field, "mode");
        assert_eq!(
            err.enum_violation[0].allowed,
            vec![Value::String("auto".into()), Value::String("manual".into())]
        );
        assert_eq!(
            err.enum_violation[0].actual,
            Value::String("unknown".into())
        );
    }

    #[test]
    fn test_validate_tool_input_collects_all_error_types() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string" },
                "content": { "type": "string" },
                "mode": { "type": "string", "enum": ["a", "b"] },
                "count": { "type": "integer" }
            },
            "required": ["file_path", "content", "mode", "count"]
        });
        let input = serde_json::json!({
            "file_path": "/x",
            "mode": "c",
            "count": "ten",
            "noise": true
        });
        let err = validate_tool_input("T", &schema, &input).unwrap_err();
        assert_eq!(err.missing_required, vec!["content".to_string()]);
        assert_eq!(err.unexpected, vec!["noise".to_string()]);
        assert_eq!(err.type_mismatch.len(), 1);
        assert_eq!(err.type_mismatch[0].field, "count");
        assert_eq!(err.enum_violation.len(), 1);
        assert_eq!(err.enum_violation[0].field, "mode");
        assert!(!err.is_empty());
    }

    #[test]
    fn test_validate_tool_input_skips_when_no_properties() {
        let schema = serde_json::json!({ "type": "object" });
        let input = serde_json::json!({ "anything": 1 });
        assert!(validate_tool_input("McpTool", &schema, &input).is_ok());
    }

    #[test]
    fn test_validate_tool_input_skips_unexpected_when_additional_properties_true() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "a": { "type": "string" } },
            "required": ["a"],
            "additionalProperties": true
        });
        let input = serde_json::json!({ "a": "x", "b": 2, "c": true });
        assert!(validate_tool_input("T", &schema, &input).is_ok());

        let input2 = serde_json::json!({ "b": 2 });
        let err = validate_tool_input("T", &schema, &input2).unwrap_err();
        assert_eq!(err.missing_required, vec!["a".to_string()]);
        assert!(err.unexpected.is_empty());

        let input3 = serde_json::json!({ "a": 1, "b": 2 });
        let err = validate_tool_input("T", &schema, &input3).unwrap_err();
        assert_eq!(err.type_mismatch.len(), 1);
        assert!(err.unexpected.is_empty());
    }

    #[test]
    fn test_validate_tool_input_skips_field_without_type() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "data": {} },
            "required": ["data"]
        });
        let input = serde_json::json!({ "data": { "any": [1, 2] } });
        assert!(validate_tool_input("T", &schema, &input).is_ok());
    }

    #[test]
    fn test_validate_tool_input_allows_null_for_nullable() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "a": { "type": "string", "nullable": true } },
            "required": ["a"]
        });
        let input = serde_json::json!({ "a": null });
        assert!(validate_tool_input("T", &schema, &input).is_ok());
    }

    #[test]
    fn test_validate_tool_input_rejects_null_when_not_nullable() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "a": { "type": "string" } },
            "required": ["a"]
        });
        let input = serde_json::json!({ "a": null });
        let err = validate_tool_input("T", &schema, &input).unwrap_err();
        assert_eq!(err.type_mismatch.len(), 1);
        assert_eq!(err.type_mismatch[0].actual, "null");
    }

    #[test]
    fn test_validate_tool_input_non_object_input_treats_as_missing_all_required() {
        let input = serde_json::json!("not an object");
        let err = validate_tool_input("Write", &write_schema(), &input).unwrap_err();
        assert_eq!(
            err.missing_required,
            vec!["file_path".to_string(), "content".to_string()]
        );
        assert!(err.unexpected.is_empty());
        assert!(err.type_mismatch.is_empty());
    }

    #[test]
    fn test_validate_tool_input_non_object_schema_skips() {
        let schema = serde_json::json!("bad schema");
        let input = serde_json::json!({ "a": 1 });
        assert!(validate_tool_input("T", &schema, &input).is_ok());
    }

    #[test]
    fn test_format_tool_input_error_lists_all_errors() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string" },
                "content": { "type": "string" },
                "count": { "type": "integer" },
                "mode": { "type": "string", "enum": ["a", "b"] }
            },
            "required": ["file_path", "content", "count", "mode"]
        });
        let input = serde_json::json!({
            "content": "hi",
            "count": "ten",
            "mode": "c",
            "noise": 1
        });
        let mismatch = validate_tool_input("Write", &schema, &input).unwrap_err();
        let msg = format_tool_input_error(&mismatch);

        assert!(msg.contains("Write"), "消息应含工具名");
        assert!(msg.contains("file_path"), "应列期望字段 file_path");
        assert!(msg.contains("content"), "应列实际字段 content");
        assert!(msg.contains("noise"), "应列多余字段 noise");
        assert!(msg.contains("count"), "应列类型不符字段 count");
        assert!(msg.contains("integer"), "应含期望类型 integer");
        assert!(msg.contains("mode"), "应列 enum 违例字段 mode");
    }

    #[test]
    fn test_format_tool_input_error_omits_empty_sections() {
        let mismatch = ToolInputMismatch {
            tool_name: "T".to_string(),
            expected: vec!["a".to_string()],
            actual: vec![],
            missing_required: vec!["a".to_string()],
            unexpected: vec![],
            type_mismatch: vec![],
            enum_violation: vec![],
        };
        let msg = format_tool_input_error(&mismatch);
        assert!(msg.contains("a"));
        assert!(!msg.contains("多余"), "无多余字段时不应出现「多余」段落");
    }
}
