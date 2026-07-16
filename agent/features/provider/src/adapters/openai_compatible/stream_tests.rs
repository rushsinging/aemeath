use crate::adapters::json_recovery::try_complete_truncated_json;
use serde_json::json;

#[test]
fn recovers_when_string_value_is_truncated_mid_quote() {
    // 模型写到 `"file_path":"/Users/...` 后流被切断
    let raw = r#"{"file_path":"/Users/x"#;
    let recovered = try_complete_truncated_json(raw).expect("应该能补全");
    assert_eq!(recovered, json!({"file_path": "/Users/x"}));
}

#[test]
fn recovers_when_string_value_contains_escape_sequences() {
    // 字符串中含有 `\"` 和 `\\` 转义，不应让状态机误判
    let raw = r#"{"content":"line1\nline2 \"with quote\""#;
    let recovered = try_complete_truncated_json(raw).expect("应该能补全");
    assert_eq!(recovered["content"], "line1\nline2 \"with quote\"");
}

#[test]
fn recovers_nested_objects() {
    // 嵌套对象，截断在最里层 string
    let raw = r#"{"outer":{"inner":{"key":"val"#;
    let recovered = try_complete_truncated_json(raw).expect("应该能补全");
    assert_eq!(recovered, json!({"outer": {"inner": {"key": "val"}}}));
}

#[test]
fn recovers_arrays_inside_object() {
    // 数组作为 value，且 array 内的 string 也被截断
    let raw = r#"{"items":["a","b","c"#;
    let recovered = try_complete_truncated_json(raw).expect("应该能补全");
    assert_eq!(recovered, json!({"items": ["a", "b", "c"]}));
}

#[test]
fn does_not_recover_when_truncated_outside_a_string() {
    // 截断在结构符之后（缺逗号），不做猜测 — 避免 silent corruption
    let raw = r#"{"a":1"#;
    assert!(try_complete_truncated_json(raw).is_none());
}

#[test]
fn does_not_recover_well_formed_json() {
    // 正常 JSON：状态机结束在 string 之外（in_string=false），不触发补全
    let raw = r#"{"a":1,"b":"ok"}"#;
    assert!(try_complete_truncated_json(raw).is_none());
}

#[test]
fn does_not_recover_when_closing_quote_would_be_invalid() {
    // 流刚好在合法 string 末尾被切，再补一个 `"` 会破坏语法；
    // 我们的状态机此时 in_string=false（最后一个 `"` 已关），所以不补。
    // 这是 expected 行为 — 这种情况下 JSON 实际上是 well-formed 的（仅缺 closing brace）。
    let raw = r#"{"a":"b""#;
    assert!(try_complete_truncated_json(raw).is_none());
}

#[test]
fn does_not_recover_completely_empty_input() {
    assert!(try_complete_truncated_json("").is_none());
}
