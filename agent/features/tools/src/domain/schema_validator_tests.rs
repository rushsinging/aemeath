use super::schema_validator::*;
use serde_json::Value;

fn write_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "file_path": { "type": "string" },
            "content": { "type": "string" }
        },
        "required": ["file_path", "content"]
    })
}

#[test]
fn valid_input_passes() {
    let input = serde_json::json!({ "file_path": "/tmp/a.txt", "content": "hello" });
    assert!(validate_tool_input("Write", &write_schema(), &input).is_ok());
}

#[test]
fn optional_field_omitted_passes() {
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
fn collects_missing_required() {
    let input = serde_json::json!({ "content": "hi" });
    let err = validate_tool_input("Write", &write_schema(), &input).unwrap_err();
    assert_eq!(err.missing_required, vec!["file_path"]);
    assert!(err.unexpected.is_empty());
}

#[test]
fn collects_all_missing_required() {
    let input = serde_json::json!({});
    let err = validate_tool_input("Write", &write_schema(), &input).unwrap_err();
    assert_eq!(err.missing_required, vec!["file_path", "content"]);
}

#[test]
fn collects_unexpected_fields() {
    let input = serde_json::json!({
        "Cow": "Borrowed(self.description())",
        "TypedTool": "...",
        "content": "hi",
        "lang": "en"
    });
    let err = validate_tool_input("Write", &write_schema(), &input).unwrap_err();
    assert_eq!(err.missing_required, vec!["file_path"]);
    let mut unexpected = err.unexpected.clone();
    unexpected.sort();
    assert_eq!(unexpected, vec!["Cow", "TypedTool", "lang"]);
}

#[test]
fn collects_type_mismatch_string_vs_number() {
    let input = serde_json::json!({ "file_path": 123, "content": "hi" });
    let err = validate_tool_input("Write", &write_schema(), &input).unwrap_err();
    assert_eq!(err.type_mismatch.len(), 1);
    assert_eq!(
        err.type_mismatch[0],
        TypeMismatch {
            field: "file_path".into(),
            expected: "string".into(),
            actual: "integer".into(),
        }
    );
}

#[test]
fn integer_rejects_float() {
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
            field: "n".into(),
            expected: "integer".into(),
            actual: "number".into(),
        }]
    );
}

#[test]
fn number_accepts_integer() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": { "x": { "type": "number" } },
        "required": ["x"]
    });
    let input = serde_json::json!({ "x": 5 });
    assert!(validate_tool_input("T", &schema, &input).is_ok());
}

#[test]
fn collects_enum_violation() {
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
}

#[test]
fn skips_when_no_properties() {
    let schema = serde_json::json!({ "type": "object" });
    let input = serde_json::json!({ "anything": 1 });
    assert!(validate_tool_input("McpTool", &schema, &input).is_ok());
}

#[test]
fn skips_unexpected_when_additional_properties_true() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": { "a": { "type": "string" } },
        "required": ["a"],
        "additionalProperties": true
    });
    let input = serde_json::json!({ "a": "x", "b": 2 });
    assert!(validate_tool_input("T", &schema, &input).is_ok());

    // required still checked
    let input2 = serde_json::json!({ "b": 2 });
    let err = validate_tool_input("T", &schema, &input2).unwrap_err();
    assert_eq!(err.missing_required, vec!["a"]);
    assert!(err.unexpected.is_empty());

    // type still checked
    let input3 = serde_json::json!({ "a": 1, "b": 2 });
    let err = validate_tool_input("T", &schema, &input3).unwrap_err();
    assert_eq!(err.type_mismatch.len(), 1);
    assert!(err.unexpected.is_empty());
}

#[test]
fn skips_field_without_type() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": { "data": {} },
        "required": ["data"]
    });
    let input = serde_json::json!({ "data": { "any": [1, 2] } });
    assert!(validate_tool_input("T", &schema, &input).is_ok());
}

#[test]
fn allows_null_for_nullable() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": { "a": { "type": "string", "nullable": true } },
        "required": ["a"]
    });
    let input = serde_json::json!({ "a": null });
    assert!(validate_tool_input("T", &schema, &input).is_ok());
}

#[test]
fn rejects_null_when_not_nullable() {
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
fn non_object_input_treats_as_missing_all_required() {
    let input = serde_json::json!("not an object");
    let err = validate_tool_input("Write", &write_schema(), &input).unwrap_err();
    assert_eq!(err.missing_required, vec!["file_path", "content"]);
    assert!(err.unexpected.is_empty());
}

#[test]
fn non_object_schema_skips() {
    let schema = serde_json::json!("bad schema");
    let input = serde_json::json!({ "a": 1 });
    assert!(validate_tool_input("T", &schema, &input).is_ok());
}

#[test]
fn format_lists_all_errors() {
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
    assert!(msg.contains("Write"));
    assert!(msg.contains("file_path"));
    assert!(msg.contains("content"));
    assert!(msg.contains("noise"));
    assert!(msg.contains("count"));
    assert!(msg.contains("integer"));
    assert!(msg.contains("mode"));
}

#[test]
fn format_omits_empty_sections() {
    let mismatch = ToolInputMismatch {
        tool_name: "T".into(),
        expected: vec!["a".into()],
        actual: vec![],
        missing_required: vec!["a".into()],
        unexpected: vec![],
        type_mismatch: vec![],
        enum_violation: vec![],
    };
    let msg = format_tool_input_error(&mismatch);
    assert!(msg.contains("a"));
    assert!(!msg.contains("多余"));
}

#[test]
fn strip_meta_removes_phase() {
    let mut input = serde_json::json!({ "skill": "commit", "phase": "execute" });
    strip_runtime_meta(&mut input);
    assert_eq!(input, serde_json::json!({ "skill": "commit" }));
}

#[test]
fn strip_meta_preserves_business_fields() {
    let mut input = serde_json::json!({
        "file_path": "/tmp/a",
        "content": "hi",
        "phase": "plan"
    });
    strip_runtime_meta(&mut input);
    assert_eq!(
        input,
        serde_json::json!({ "file_path": "/tmp/a", "content": "hi" })
    );
}

#[test]
fn strip_meta_non_object_is_noop() {
    let mut input = serde_json::json!("not an object");
    strip_runtime_meta(&mut input);
    assert_eq!(input, serde_json::json!("not an object"));
}

#[test]
fn validate_passes_after_stripping_phase() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "skill": { "type": "string" },
            "args": { "type": "string" }
        },
        "required": ["skill"]
    });
    let mut input = serde_json::json!({ "skill": "commit", "phase": "execute" });
    strip_runtime_meta(&mut input);
    assert!(validate_tool_input("Skill", &schema, &input).is_ok());
}

#[test]
fn validate_rejects_phase_when_not_stripped() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "skill": { "type": "string" },
            "args": { "type": "string" }
        },
        "required": ["skill"]
    });
    let input = serde_json::json!({ "skill": "commit", "phase": "execute" });
    let err = validate_tool_input("Skill", &schema, &input).unwrap_err();
    assert_eq!(err.unexpected, vec!["phase"]);
}
