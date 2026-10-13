//! 结构化 tool_result 在 TUI 预览限额下的行为契约（issue #1957）。
//!
//! 契约：超限时 **NEVER** 把整个 JSON 降级为截断字符串；对已知结构化工具
//! （ToolSearch 等）必须保持对象形状，只逐条截断大字段并按预算收敛，
//! 保证 `format_result_lines` 仍能反序列化并渲染结构化行。

use super::super::constants::{TOOL_RESULT_PREVIEW_LIMIT, TOOL_SEARCH_DESCRIPTION_BUDGET_CHARS};
use super::sanitize_tool_result_content;
use serde_json::{json, Value};

use sdk::tool_result::{SearchItemKind, ToolSearchResult};

/// 造 `count` 条接近真实体积的 ToolSearch 结果（每条约 250B）。
fn tool_search_result(count: usize) -> Value {
    let tools: Vec<Value> = (0..count)
        .map(|index| {
            json!({
                "name": format!("tool_{index}"),
                "description": "x".repeat(180),
                "input_schema": {"type": "object", "properties": {}},
                "is_read_only": true,
                "kind": "tool",
            })
        })
        .collect();
    json!({ "tools": tools })
}

fn deserialize(content: &Value) -> ToolSearchResult {
    serde_json::from_value(content.clone()).expect("结构化结果必须在预览收敛后仍可反序列化")
}

#[test]
fn tool_search_within_raised_limit_keeps_every_item() {
    let raw = tool_search_result(95);
    let bytes = raw.to_string().len();
    assert!(
        bytes > 16 * 1024,
        "前置条件：结果应超过旧 16KB 限额，实际 {bytes}"
    );
    assert!(
        bytes <= TOOL_RESULT_PREVIEW_LIMIT,
        "前置条件：结果应在 64KB 限额内，实际 {bytes}"
    );

    let sanitized = sanitize_tool_result_content("ToolSearch", raw.clone());

    assert_eq!(
        sanitized, raw,
        "限额提升到 64KB 后，95 条结果应原样保留（不裁剪、不标省略）"
    );
}

#[test]
fn oversized_tool_search_keeps_object_shape() {
    let raw = tool_search_result(300);
    let bytes = raw.to_string().len();
    assert!(
        bytes > TOOL_RESULT_PREVIEW_LIMIT,
        "前置条件：构造结果应超过 64KB 限额，实际 {bytes}"
    );

    let sanitized = sanitize_tool_result_content("ToolSearch", raw);

    assert!(
        sanitized.is_object(),
        "超限后仍应是 JSON 对象，实际：{sanitized}"
    );
    assert!(
        sanitized.to_string().len() <= TOOL_RESULT_PREVIEW_LIMIT,
        "收敛后体积应回到限额内，实际 {}",
        sanitized.to_string().len()
    );

    let result = deserialize(&sanitized);
    assert!(
        !result.tools.is_empty() && result.tools.len() < 300,
        "应按 64KB 预算保留最大可容纳前缀，实际 {}",
        result.tools.len()
    );
    let marker = sanitized["tools_truncated"]
        .as_str()
        .expect("应保留条数省略标记");
    let (total, omitted) = marker
        .split_once('/')
        .map(|(total, omitted)| (total.to_owned(), omitted.to_owned()))
        .expect("省略标记格式应为 total/omitted");
    assert_eq!(total, "300");
    assert_eq!(omitted, (300 - result.tools.len()).to_string());
    assert_eq!(result.tools[0].name, "tool_0", "保留前 N 条的顺序");
}

#[test]
fn oversized_tool_description_is_truncated_per_item() {
    let mut raw = tool_search_result(300);
    raw["tools"][0]["description"] = json!("d".repeat(4000));
    assert!(
        raw.to_string().len() > TOOL_RESULT_PREVIEW_LIMIT,
        "前置条件：含超长描述的结果应超过 64KB 限额"
    );

    let sanitized = sanitize_tool_result_content("ToolSearch", raw);
    assert!(sanitized.is_object(), "单条超长描述不应整块降级");

    let result = deserialize(&sanitized);
    let description = &result.tools[0].description;
    assert!(
        description.chars().count() <= TOOL_SEARCH_DESCRIPTION_BUDGET_CHARS + 64,
        "单条描述应被逐条截断，实际 {} 字符",
        description.chars().count()
    );
    assert!(
        description.contains("omitted"),
        "截断处应带省略标记，实际：{description}"
    );
    assert_eq!(result.tools[0].name, "tool_0", "name 字段 NEVER 丢失");
    assert_eq!(result.tools[0].kind, SearchItemKind::Tool);
    assert!(result.tools[0].is_read_only);
}

#[test]
fn within_limit_tool_search_is_unchanged() {
    let raw = tool_search_result(5);
    assert!(raw.to_string().len() <= TOOL_RESULT_PREVIEW_LIMIT);

    let sanitized = sanitize_tool_result_content("ToolSearch", raw.clone());
    assert_eq!(sanitized, raw, "未超限时不应改写结构化结果");
}

#[test]
fn unregistered_tool_truncates_long_fields_but_keeps_shape() {
    let raw = json!({ "payload": "y".repeat(TOOL_RESULT_PREVIEW_LIMIT + 1) });
    let sanitized = sanitize_tool_result_content("SomeMcpTool", raw);

    assert!(
        sanitized.is_object(),
        "长字符串字段应被逐字段截断而不是整块降级，实际：{sanitized}"
    );
    let payload = sanitized["payload"]
        .as_str()
        .expect("payload 字段应保留为字符串");
    assert!(payload.contains("chars omitted from TUI preview)"));
    assert!(
        sanitized.to_string().len() <= TOOL_RESULT_PREVIEW_LIMIT,
        "收敛后体积应回到限额内"
    );
}

#[test]
fn unconvergent_shape_still_degrades_to_string() {
    let items: Vec<Value> = (0..20_000).map(|_| json!("x")).collect();
    let raw = json!({ "items": items });
    assert!(raw.to_string().len() > TOOL_RESULT_PREVIEW_LIMIT);

    let sanitized = sanitize_tool_result_content("SomeMcpTool", raw);

    assert!(
        sanitized.is_string(),
        "字段截断无法收敛时仍保留整块降级的兜底路径"
    );
}

/// L2：超限结果经 sanitize 后，显示层仍能渲染结构化行（NEVER 回退原始文本）。
#[test]
fn sanitized_oversized_result_still_renders_structured_lines() {
    use crate::tui::render::output::tool_display::lookup_display;

    let raw = tool_search_result(300);
    let sanitized = sanitize_tool_result_content("ToolSearch", raw);

    let display = lookup_display("ToolSearch").expect("ToolSearch 已注册 ToolDisplay");
    let lines = display
        .format_result_lines(Some(&sanitized))
        .expect("sanitize 后仍应产出 typed 行");

    assert!(
        lines.len() > 15,
        "超限收敛后仍应产出足量结构化行，实际 {}",
        lines.len()
    );
    let first = &lines[0];
    assert!(
        first.contains("tool ·") || first.starts_with("tool "),
        "首行：{first}"
    );
    assert!(first.contains("tool_0"), "首行应含首个工具名：{first}");
    assert!(
        !first.contains("Found 95 tool(s)"),
        "NEVER 回退为原始文本列表：{first}"
    );
}
