//! RSI 预览/事件脱敏。
//!
//! 证据 signal id 会内嵌工具参数（例如 Bash command）。预览与事件属于
//! 用户可见/可持久化面，MUST 在写出前遮蔽常见凭据；内部匹配仍使用原文。

/// 遮蔽 `sk-*` 与 `Bearer <token>` 形态的凭据。
pub(crate) fn redact_sensitive_text(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        let let_some_rest = text.get(index..);
        let Some(rest) = let_some_rest else {
            break;
        };
        if rest.starts_with("sk-") {
            output.push_str("sk-***");
            index += 3;
            skip_secret_chars(text, &mut index);
            continue;
        }
        if rest.len() >= 7
            && rest
                .get(..7)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("bearer "))
        {
            output.push_str("Bearer ***");
            index += 7;
            skip_secret_chars(text, &mut index);
            continue;
        }
        let Some(ch) = rest.chars().next() else {
            break;
        };
        output.push(ch);
        index += ch.len_utf8();
    }
    output
}

fn skip_secret_chars(text: &str, index: &mut usize) {
    while *index < text.len() {
        let Some(ch) = text.get(*index..).and_then(|rest| rest.chars().next()) else {
            break;
        };
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '+' | '/' | '=') {
            *index += ch.len_utf8();
        } else {
            break;
        }
    }
}

/// 递归遮蔽 JSON 事件中的全部字符串值。
pub(crate) fn redact_json_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) => {
            *text = redact_sensitive_text(text);
        }
        serde_json::Value::Array(values) => {
            for value in values {
                redact_json_value(value);
            }
        }
        serde_json::Value::Object(map) => {
            for value in map.values_mut() {
                redact_json_value(value);
            }
        }
        _ => {}
    }
}
