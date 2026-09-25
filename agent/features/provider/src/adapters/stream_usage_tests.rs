//! #1679 行为测试：承接 check-provider-usage-capability 守卫退役。
//! RawUsage 解析必须保留缺失值（None），禁抹零与 u64→u32 截断。

use super::anthropic_raw_usage_from_line;

fn message_start_usage(usage_json: &str) -> String {
    format!(r#"{{"type":"message_start","message":{{"usage":{usage_json}}}}}"#)
}

#[test]
fn missing_usage_fields_stay_none_instead_of_zero() {
    let line = message_start_usage(r#"{"input_tokens":7}"#);
    let snapshot = anthropic_raw_usage_from_line(&line).expect("usage line must parse");

    assert_eq!(snapshot.input_tokens, Some(7));
    assert_eq!(
        snapshot.output_tokens, None,
        "缺失字段必须保留 None，禁抹零"
    );
    assert_eq!(snapshot.cache_read_tokens, None);
    assert_eq!(snapshot.cache_write_tokens, None);
    assert!(!snapshot.was_reported() || snapshot.input_tokens.is_some());
}

#[test]
fn out_of_u32_range_values_stay_none_instead_of_truncating() {
    // u64 上界超出 u32：禁 `as u32` 截断，必须整体视为不可信（None）。
    let line = message_start_usage(r#"{"input_tokens":8589934592,"output_tokens":3}"#);
    let snapshot = anthropic_raw_usage_from_line(&line).expect("usage line must parse");

    assert_eq!(
        snapshot.input_tokens, None,
        "超 u32 值必须 None，禁 as u32 截断"
    );
    assert_eq!(snapshot.output_tokens, Some(3));
}

#[test]
fn null_usage_field_stays_none() {
    let line = message_start_usage(r#"{"input_tokens":null,"output_tokens":5}"#);
    let snapshot = anthropic_raw_usage_from_line(&line).expect("usage line must parse");

    assert_eq!(snapshot.input_tokens, None, "null 字段必须 None");
    assert_eq!(snapshot.output_tokens, Some(5));
}
