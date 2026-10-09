//! 用户输入时间戳的 LLM 视图渲染与摘要侧剥离。
//!
//! 真实用户输入在构造时由 `MessageMetadata::created_at` 盖章；Context window
//! 渲染时据此生成 LLM 时间前缀，canonical 文本与落盘 JSON 不消费。compact
//! 摘要只吸收对话语义，时间前缀属渲染层噪声，进入摘要前必须剥离。

use chrono::{DateTime, FixedOffset};

use super::constants::USER_INPUT_TIMESTAMP_FORMAT;

/// 渲染 LLM 视图的 user 输入时间前缀 `[YYYY-MM-DD HH:MM ±ZZZZ]: `。
pub fn render_user_input_timestamp_prefix(created_at: &DateTime<FixedOffset>) -> String {
    format!("[{}]: ", created_at.format(USER_INPUT_TIMESTAMP_FORMAT))
}

/// 剥离 [`render_user_input_timestamp_prefix`] 产生的时间前缀。
///
/// 幂等：无前缀或形态不匹配时原样返回。校验采用 round-trip——按同一格式
/// 解析后回写必须与原文逐字一致——保证与渲染侧共用唯一格式真源，且不会
/// 误伤普通方括号文本（如 `[note]: ...`）。
pub fn strip_user_input_timestamp_prefix(text: &str) -> &str {
    let Some(rest) = text.strip_prefix('[') else {
        return text;
    };
    let Some((stamp, body)) = rest.split_once("]: ") else {
        return text;
    };
    match DateTime::parse_from_str(stamp, USER_INPUT_TIMESTAMP_FORMAT) {
        Ok(parsed) if parsed.format(USER_INPUT_TIMESTAMP_FORMAT).to_string() == stamp => body,
        _ => text,
    }
}
