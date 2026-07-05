//! 截断 JSON 的启发式恢复。
//!
//! 上游 SSE 流在 tool_call `arguments` 字符串字面量中间被截断时，
//! 通过补全 closing quote 和必要的右括号尝试恢复出可解析的 JSON。

/// 尝试从上游截断的 JSON 字符串中恢复出可解析的 JSON。
///
/// 适用场景：上游 SSE 流在某个 tool_call `arguments` 字符串字面量中间被截断（典型 EOF 错误）。
/// 该情况是 provider 最常见的流式截断形态，因为模型经常在 string 边界被切。
///
/// 启发式策略：
/// 1. 用状态机扫描原始字符串，跟踪"是否在 string 内"（正确处理 `\\` 和 `\"` 转义）。
/// 2. 仅当流结束**且**仍处于 string 中时尝试补全；其他截断形态（如缺逗号/冒号）不做猜测。
/// 3. 补 `"` 关闭 string，然后按未闭合的结构符顺序补 `}` / `]`。
/// 4. 重新调用 `serde_json::from_str`；成功则返回 `Some(value)`，失败返回 `None`（让 caller 走原错误路径）。
///
/// 注意：**绝不**对截断在结构边界（`,` `:` `{` `[` 之后）的情况做"猜测式补全"，
/// 因为那会引入 silent corruption（例如把 `{"a":1` 补成 `{"a":1}`，模型侧的语义可能完全不同）。
pub fn try_complete_truncated_json(raw: &str) -> Option<serde_json::Value> {
    let mut in_string = false;
    let mut escape = false;
    for &b in raw.as_bytes() {
        if escape {
            escape = false;
            continue;
        }
        match b {
            b'\\' if in_string => escape = true,
            b'"' => in_string = !in_string,
            _ => {}
        }
    }

    // 只处理"在 string 内被截断"这一种形态。其他形态让 caller 抛错。
    if !in_string {
        return None;
    }

    // 补一个 closing quote，然后遍历整段字符串统计未闭合的结构符。
    let mut candidate = String::with_capacity(raw.len() + 16);
    candidate.push_str(raw);
    candidate.push('"');

    let mut stack: Vec<u8> = Vec::new();
    let mut in_str2 = false;
    let mut esc2 = false;
    for &b in candidate.as_bytes() {
        if esc2 {
            esc2 = false;
            continue;
        }
        match b {
            b'\\' if in_str2 => esc2 = true,
            b'"' => in_str2 = !in_str2,
            b'{' if !in_str2 => stack.push(b'}'),
            b'[' if !in_str2 => stack.push(b']'),
            _ => {}
        }
    }

    // 按栈的逆序补右括号（即最深层的先闭合）。
    while let Some(c) = stack.pop() {
        candidate.push(c as char);
    }

    serde_json::from_str(&candidate).ok()
}
