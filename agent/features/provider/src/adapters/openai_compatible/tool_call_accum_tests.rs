//! tool_calls 分片合并语义测试。
//!
//! 各 OpenAI 兼容网关对「未提供的字段」有两种表达方式：**缺失字段**（Aliyun）
//! 或**空串**（Qiniu）。两者语义等价，都必须视为「本帧未提供」，不得覆盖既有值。

use super::ToolCallAccum;

fn delta(value: serde_json::Value) -> serde_json::Value {
    value
}

/// Qiniu 真实分片序列：首帧带 id/name，后续每帧 `"id":""`、`"name":""` 只带
/// arguments 片段。空串不得冲掉首帧记录的真实 id。
#[test]
fn empty_id_delta_keeps_real_id_from_first_frame() {
    let mut accum = ToolCallAccum::default();

    accum.merge(
        0,
        &delta(serde_json::json!({
            "index": 0,
            "id": "call_00_7p3cnvmt8hgxsyfshs6smgux",
            "function": {"name": "Bash"}
        })),
    );
    accum.merge(
        0,
        &delta(serde_json::json!({
            "index": 0,
            "id": "",
            "function": {"name": "", "arguments": "{\"command\":"}
        })),
    );
    accum.merge(
        0,
        &delta(serde_json::json!({
            "index": 0,
            "id": "",
            "function": {"name": "", "arguments": "\"echo hi\"}"}
        })),
    );

    assert_eq!(accum.id, "call_00_7p3cnvmt8hgxsyfshs6smgux");
    assert_eq!(accum.name, "Bash");
    assert_eq!(accum.arguments, "{\"command\":\"echo hi\"}");
    assert_eq!(accum.delta_count, 2);
}

/// Aliyun 形态：后续帧缺少 id 字段（而不是空串），同样不得覆盖。
#[test]
fn absent_id_field_keeps_real_id_from_first_frame() {
    let mut accum = ToolCallAccum::default();

    accum.merge(
        0,
        &delta(serde_json::json!({
            "index": 0,
            "id": "call_149ac8bd1f3e4237b2ad2a2e",
            "function": {"name": "Bash", "arguments": ""}
        })),
    );
    accum.merge(
        0,
        &delta(serde_json::json!({
            "index": 0,
            "function": {"arguments": "{\"command\":\"echo hi\"}"}
        })),
    );

    assert_eq!(accum.id, "call_149ac8bd1f3e4237b2ad2a2e");
    assert_eq!(accum.arguments, "{\"command\":\"echo hi\"}");
}

/// 全程无 id 的 provider 必须得到稳定兜底 id，否则多路 tool call 会互相串错。
#[test]
fn missing_id_throughout_falls_back_to_index_scoped_id() {
    let mut accum = ToolCallAccum::default();

    accum.merge(
        3,
        &delta(serde_json::json!({
            "index": 3,
            "function": {"name": "Read", "arguments": "{}"}
        })),
    );
    accum.ensure_id(3);

    assert_eq!(accum.id, "call_3");
}

/// 并行多路 tool call：各自按 index 隔离兜底，id 必须互不相同。
#[test]
fn parallel_tool_calls_without_id_get_distinct_ids() {
    let mut first = ToolCallAccum::default();
    let mut second = ToolCallAccum::default();

    first.merge(
        0,
        &delta(serde_json::json!({"index": 0, "function": {"name": "Read"}})),
    );
    second.merge(
        1,
        &delta(serde_json::json!({"index": 1, "function": {"name": "Glob"}})),
    );
    first.ensure_id(0);
    second.ensure_id(1);

    assert_eq!(first.id, "call_0");
    assert_eq!(second.id, "call_1");
    assert_ne!(first.id, second.id);
}

/// 已有真实 id 时兜底不得覆盖它。
#[test]
fn ensure_id_preserves_existing_real_id() {
    let mut accum = ToolCallAccum::default();

    accum.merge(
        0,
        &delta(serde_json::json!({
            "index": 0,
            "id": "call_real",
            "function": {"name": "Bash"}
        })),
    );
    accum.ensure_id(0);

    assert_eq!(accum.id, "call_real");
}

/// name 只在首次非空出现时标记为「新增」，供 UI 实时建卡。
#[test]
fn name_reports_new_only_on_first_non_empty_value() {
    let mut accum = ToolCallAccum::default();

    let first = accum.merge(
        0,
        &delta(serde_json::json!({"index": 0, "function": {"name": "Bash"}})),
    );
    let second = accum.merge(
        0,
        &delta(serde_json::json!({"index": 0, "function": {"name": "Bash"}})),
    );
    let third = accum.merge(
        0,
        &delta(serde_json::json!({"index": 0, "function": {"name": ""}})),
    );

    assert!(first.name_is_new, "首次非空 name 应报告为新增，供 UI 建卡");
    assert!(!second.name_is_new, "重复 name 不应重复报告");
    assert!(!third.name_is_new, "空串 name 不应报告");
    assert_eq!(accum.name, "Bash");
}

/// `has_arguments` 只在真正携带 arguments 增量时为真，
/// 避免无参数的帧重复触发 UI 参数预览推送。
#[test]
fn has_arguments_reflects_presence_of_arguments_field() {
    let mut accum = ToolCallAccum::default();

    let with_name_only = accum.merge(
        0,
        &delta(serde_json::json!({"index": 0, "function": {"name": "Bash"}})),
    );
    let with_args = accum.merge(
        0,
        &delta(serde_json::json!({"index": 0, "function": {"arguments": "{}"}})),
    );
    let with_empty_id_only = accum.merge(
        0,
        &delta(serde_json::json!({"index": 0, "id": "", "function": {"name": ""}})),
    );

    assert!(!with_name_only.has_arguments, "仅 name 的帧不应标记带参数");
    assert!(with_args.has_arguments, "携带 arguments 的帧应标记");
    assert!(!with_empty_id_only.has_arguments, "空壳帧不应标记带参数");
}

/// 首帧 id 为空串、后续帧才给出真实 id 时，应采纳后到的真实 id。
#[test]
fn late_real_id_overrides_empty_placeholder() {
    let mut accum = ToolCallAccum::default();

    accum.merge(
        0,
        &delta(serde_json::json!({"index": 0, "id": "", "function": {"name": "Bash"}})),
    );
    accum.merge(
        0,
        &delta(serde_json::json!({
            "index": 0,
            "id": "call_late",
            "function": {"arguments": "{}"}
        })),
    );

    assert_eq!(accum.id, "call_late");
}
