//! Tool input schema pre-validation (moved from runtime into Tools BC).
//!
//! Validates a `serde_json::Value` input against a JSON Schema before tool
//! dispatch. Collects **all** errors (missing required / unexpected / type
//! mismatch / enum violation) in a single pass so the LLM can correct
//! everything in one round.
//!
//! Design points:
//! - Collect-all rather than fail-fast.
//! - Loose schema graceful degradation: no `properties` → skip entirely;
//!   `additionalProperties: true` → skip unexpected-field checks only
//!   (required/type/enum checks still run).
//! - Field schema without `type` (e.g. `Value`-generated `{}`) → skip type
//!   check.
//! - `nullable: true` fields accept `null`.

use serde_json::Value;

/// Runtime metadata keys injected by the system prompt (e.g. `phase` for
/// reasoning graph classification). These are NOT part of any tool's business
/// schema and MUST be stripped before validation/dispatch, otherwise strictly-
/// schemad tools (no `additionalProperties`) will reject them as "unexpected
/// fields" (issue #491).
pub const RUNTIME_META_KEYS: &[&str] = &["phase"];

/// Remove runtime meta keys from a tool input in-place.
///
/// Only operates on object-typed inputs; non-objects (string/array) pass
/// through unchanged.
pub fn strip_runtime_meta(input: &mut Value) {
    let obj = match input.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    for key in RUNTIME_META_KEYS {
        obj.remove(*key);
    }
}

// ── Error types ──────────────────────────────────────────────────────

/// A single type-mismatch record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeMismatch {
    pub field: String,
    pub expected: String,
    pub actual: String,
}

/// A single enum-violation record.
#[derive(Debug, Clone, PartialEq)]
pub struct EnumViolation {
    pub field: String,
    pub allowed: Vec<Value>,
    pub actual: Value,
}

/// All errors collected during tool-input validation.
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

// ── Public API ───────────────────────────────────────────────────────

/// Validate a tool input against the tool's JSON Schema.
///
/// Returns `Ok(())` when the input is valid. Returns `Err(Box<ToolInputMismatch>)`
/// containing all collected errors.
pub fn validate_tool_input(
    tool_name: &str,
    schema: &Value,
    input: &Value,
) -> Result<(), Box<ToolInputMismatch>> {
    // Loose schema without `properties` — skip entirely.
    let properties = match schema.get("properties").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => return Ok(()),
    };

    // `additionalProperties: true` only skips the unexpected-field check;
    // required/type/enum checks still apply.
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
        // Non-object input: treat as missing all required fields.
        None => {
            mismatch.actual = Vec::new();
            mismatch.missing_required = required;
            return Err(Box::new(mismatch));
        }
    };

    mismatch.actual = input_obj.keys().cloned().collect();

    // Missing required fields.
    for req in &required {
        if !input_obj.contains_key(req) {
            mismatch.missing_required.push(req.clone());
        }
    }

    // Inspect each actual field.
    for (key, value) in input_obj {
        match properties.get(key) {
            None => {
                if !allow_additional {
                    mismatch.unexpected.push(key.clone());
                }
            }
            Some(field_schema) => {
                // Type check first; if type is wrong, skip enum check
                // (type error is the more fundamental problem).
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

/// Format a `ToolInputMismatch` into a human-readable Chinese error message.
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

// ── Internal helpers ──────────────────────────────────────────────────

/// Check whether a field value's type matches the schema.
///
/// Returns `Some((expected, actual))` when there's a mismatch.
///
/// Skips:
/// - Field schema with no `type` (e.g. `{}` from `Value` fields) → `None`.
/// - `nullable: true` + value is `null` → `None`.
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
        _ => true, // unknown type — don't validate
    };
    if matches {
        None
    } else {
        Some((expected.to_string(), actual))
    }
}

/// Return the JSON type name of a `serde_json::Value`, distinguishing
/// `integer` from `number`.
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

// ── Tests ─────────────────────────────────────────────────────────────
