use super::constants::{
    STRUCTURED_MARKER_RESERVE_BYTES, TOOL_LARGE_FIELD_PREVIEW_LIMIT, TOOL_RESULT_PREVIEW_LIMIT,
    TOOL_SEARCH_DESCRIPTION_BUDGET_CHARS, TOOL_SEARCH_ITEM_BUDGET, TOOL_STREAM_PREVIEW_LIMIT,
    TUI_OMISSION_SUFFIX_MARKER,
};
use crate::tui::text::safe_str_slice_by_char;
use serde_json::{Map, Value};

// ════════════════════════════════════════════════════════════════════
//  Helpers — tool output sanitization (inlined from tool_flow_projector)
// ════════════════════════════════════════════════════════════════════

pub(super) fn sanitize_tool_arguments_delta(tool_name: &str, partial_args: &str) -> String {
    match serde_json::from_str::<Value>(partial_args) {
        Ok(value) => {
            // 对大字段做摘要后重新序列化，保持 JSON 有效性。
            // 不再做字节截断：大字段已被 summarize_object_string_field 控制在 256 字节以内，
            // 其余字段通常很短，整体 JSON 不会过大。
            sanitize_tool_value(tool_name, value).to_string()
        }
        Err(_) => truncate_tool_text(partial_args, TOOL_STREAM_PREVIEW_LIMIT, Some(tool_name)),
    }
}

pub(super) fn sanitize_tool_output(tool_name: &str, output: &str) -> String {
    truncate_large_tool_text(output, Some(tool_name))
}

pub(super) fn sanitize_tool_result_content(tool_name: &str, content: Value) -> Value {
    let sanitized = match content {
        Value::Object(object) => sanitize_tool_value(tool_name, Value::Object(object)),
        value => truncate_json_value(value, tool_name, "content"),
    };
    if sanitized.to_string().len() <= TOOL_RESULT_PREVIEW_LIMIT {
        return sanitized;
    }
    // #1957：已登记结构化形状的工具**结构保留**收敛，NEVER 整块降级为字符串
    // （否则 TUI `format_result_lines` 解析失败、退化成原始文本列表）。
    if let Some(structured) = truncate_structured_object(tool_name, sanitized.clone()) {
        return structured;
    }
    truncate_json_value(sanitized, tool_name, "content")
}

/// 对结构化 tool_result 做「保结构」收敛；无法在限额内收敛时返回 `None`。
///
/// - `ToolSearch`：专属策略（逐条截描述 + 条目预算 + 省略标记）。
/// - 其他工具：通用策略——只截长字符串字段，保持对象/数组形状。
fn truncate_structured_object(tool_name: &str, value: Value) -> Option<Value> {
    if tool_name == "ToolSearch" {
        return Some(truncate_tool_search_result(value));
    }
    let truncated = truncate_long_string_fields(value);
    (truncated.to_string().len() <= TOOL_RESULT_PREVIEW_LIMIT).then_some(truncated)
}

/// 递归截断长字符串字段（幂等：已带 TUI 省略标记的字符串不再二次截断）。
fn truncate_long_string_fields(value: Value) -> Value {
    match value {
        Value::String(text) => truncate_long_string(text),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(truncate_long_string_fields).collect())
        }
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, item)| (key, truncate_long_string_fields(item)))
                .collect(),
        ),
        other => other,
    }
}

fn truncate_long_string(text: String) -> Value {
    if text.chars().count() <= TOOL_LARGE_FIELD_PREVIEW_LIMIT
        || text.ends_with(TUI_OMISSION_SUFFIX_MARKER)
    {
        return Value::String(text);
    }
    let prefix: String = text.chars().take(TOOL_LARGE_FIELD_PREVIEW_LIMIT).collect();
    let omitted = text.chars().count() - TOOL_LARGE_FIELD_PREVIEW_LIMIT;
    Value::String(format!(
        "{prefix} ... ({omitted} chars omitted from TUI preview)"
    ))
}

/// ToolSearch 结果：逐条截 `description` → 收敛条目数 → 保留省略标记。
///
/// 输出始终是合法 JSON 对象（形状 `{"tools":[…], "tools_truncated"?: "total/omitted"}`），
/// 保证下游 `ToolSearchResult` 反序列化与结构化行渲染不失效。
fn truncate_tool_search_result(value: Value) -> Value {
    let Value::Object(mut object) = value else {
        return truncate_json_value(value, "ToolSearch", "content");
    };
    let Some(tools) = object.remove("tools") else {
        return Value::Object(object);
    };
    let Value::Array(tools) = tools else {
        return Value::Object(object);
    };
    let total = tools.len();
    let truncated: Vec<Value> = tools
        .into_iter()
        .take(TOOL_SEARCH_ITEM_BUDGET)
        .map(truncate_tool_search_item)
        .collect();

    object.insert("tools".to_owned(), Value::Array(Vec::new()));
    let mut kept = truncated;
    while kept.len() > 1 && measured_len(&object, &kept) > TOOL_RESULT_PREVIEW_LIMIT {
        kept.pop();
    }
    let omitted = total.saturating_sub(kept.len());
    object.insert("tools".to_owned(), Value::Array(kept));
    if omitted > 0 {
        object.insert(
            "tools_truncated".to_owned(),
            Value::String(format!("{total}/{omitted}")),
        );
    }
    Value::Object(object)
}

/// 单条 ToolSearch 条目：只截长描述，其余结构化字段原样保留。
fn truncate_tool_search_item(tool: Value) -> Value {
    let Value::Object(mut item) = tool else {
        return tool;
    };
    let Some(Value::String(description)) = item.get("description").cloned() else {
        return Value::Object(item);
    };
    if description.chars().count() <= TOOL_SEARCH_DESCRIPTION_BUDGET_CHARS {
        return Value::Object(item);
    }
    let prefix: String = description
        .chars()
        .take(TOOL_SEARCH_DESCRIPTION_BUDGET_CHARS)
        .collect();
    let omitted = description.chars().count() - TOOL_SEARCH_DESCRIPTION_BUDGET_CHARS;
    item.insert(
        "description".to_owned(),
        Value::String(format!(
            "{prefix} ... ({omitted} chars omitted from TUI ToolSearch preview)"
        )),
    );
    Value::Object(item)
}

/// 以「对象骨架 + 候选 tools」测量体积（含省略标记余量）。
fn measured_len(object: &Map<String, Value>, tools: &[Value]) -> usize {
    let mut probe = object.clone();
    probe.insert("tools".to_owned(), Value::Array(tools.to_vec()));
    Value::Object(probe).to_string().len() + STRUCTURED_MARKER_RESERVE_BYTES
}

fn sanitize_tool_value(tool_name: &str, value: Value) -> Value {
    let Value::Object(mut object) = value else {
        return truncate_json_value(value, tool_name, "value");
    };
    for field in large_fields_for_tool(tool_name) {
        summarize_object_string_field(&mut object, tool_name, field);
    }
    Value::Object(object)
}

fn large_fields_for_tool(tool_name: &str) -> &'static [&'static str] {
    match tool_name {
        "Write" => &["content"],
        "Edit" => &["old_string", "new_string"],
        "Agent" => &["prompt"],
        "Bash" => &["command"],
        "AskUserQuestion" => &["question"],
        _ => &[],
    }
}

fn summarize_object_string_field(object: &mut Map<String, Value>, tool_name: &str, field: &str) {
    let Some(value) = object.get_mut(field) else {
        return;
    };
    let Some(text) = value.as_str() else {
        return;
    };
    if text.len() <= TOOL_LARGE_FIELD_PREVIEW_LIMIT {
        return;
    }
    *value = Value::String(format!(
        "{} ... ({} bytes omitted from TUI {tool_name}.{field} preview)",
        utf8_prefix(text, TOOL_LARGE_FIELD_PREVIEW_LIMIT),
        text.len()
            .saturating_sub(utf8_prefix(text, TOOL_LARGE_FIELD_PREVIEW_LIMIT).len())
    ));
}

fn truncate_json_value(value: Value, tool_name: &str, field: &str) -> Value {
    let text = value.to_string();
    Value::String(truncate_tool_text(
        &text,
        TOOL_RESULT_PREVIEW_LIMIT,
        Some(&format!("{tool_name}.{field}")),
    ))
}

fn truncate_large_tool_text(text: &str, context: Option<&str>) -> String {
    truncate_tool_text(text, TOOL_RESULT_PREVIEW_LIMIT, context)
}

fn truncate_tool_text(text: &str, limit: usize, context: Option<&str>) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let prefix = utf8_prefix(text, limit);
    let omitted = text.len().saturating_sub(prefix.len());
    let suffix = match context {
        Some(context) => format!("... ({omitted} bytes omitted from TUI preview for {context})"),
        None => format!("... ({omitted} bytes omitted from TUI preview)"),
    };
    format!("{prefix}\n{suffix}")
}

fn utf8_prefix(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let char_end = text
        .char_indices()
        .take_while(|(idx, ch)| idx + ch.len_utf8() <= limit)
        .count();
    safe_str_slice_by_char(text, 0, char_end)
}

#[cfg(test)]
#[path = "sanitize_tests.rs"]
mod tests;
