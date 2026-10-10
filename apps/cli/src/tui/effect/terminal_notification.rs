//! OSC 777（RXVT notify）终端桌面通知：上下文组装、序列格式化与写出。
//!
//! 回合完成时向终端写 `\x1b]777;notify;<title>;<body>\x07`，由终端自身决定
//! 是否弹桌面提醒（cmux/iTerm2 在窗口聚焦时自行抑制）；对不支持 OSC 777 的
//! 终端该序列无害，**NEVER** 做终端能力猜测。
//!
//! 通知携带 session 上下文：
//! - title = `aemeath · <项目名末两段>`（项目名缺失时回退 `aemeath`）
//! - body  = `Turn complete in <耗时> · <分支> · <prompt 首行>`（完成状态在
//!   最前，横幅截断也不丢核心信息；缺失段省略）
//!
//! 长度防护：prompt 首行超 60 字符截断；长路径 title 只保留末两段。

use super::constants::PROMPT_SUMMARY_MAX_CHARS;
use std::io;

use crate::tui::model::output_timeline::OutputTimelineItem;

#[cfg(test)]
#[path = "terminal_notification_tests.rs"]
mod tests;

/// 回合完成通知的 session 上下文；`None`/空的段在组装时省略。
pub(crate) struct TurnCompleteNotificationContext<'a> {
    /// 当前回合的用户 prompt 摘要（已提取、清洗）。
    pub(crate) prompt: Option<&'a str>,
    /// 项目目录名（workspace path_base）。
    pub(crate) path_base: Option<&'a str>,
    /// git 分支名。
    pub(crate) branch: Option<&'a str>,
    /// 回合耗时。
    pub(crate) duration_ms: Option<u64>,
}

/// 从 timeline 提取当前 prompt：最后一条真实用户消息的首行摘要。
///
/// 跳过纯 `system-reminder` 注入（hook/skill 内部消息），无用户消息时返回 None。
pub(crate) fn latest_user_prompt(items: &[OutputTimelineItem]) -> Option<String> {
    items.iter().rev().find_map(|item| match item {
        OutputTimelineItem::UserMessage { text, .. } => summarize_prompt(text),
        _ => None,
    })
}

/// prompt → 首行摘要：取首行、trim、超长截断；空首行返回 None。
fn summarize_prompt(text: &str) -> Option<String> {
    let first_line = text.lines().next().unwrap_or_default().trim();
    if first_line.is_empty() {
        return None;
    }
    if first_line.chars().count() > PROMPT_SUMMARY_MAX_CHARS {
        let truncated: String = first_line.chars().take(PROMPT_SUMMARY_MAX_CHARS).collect();
        Some(format!("{truncated}…"))
    } else {
        Some(first_line.to_string())
    }
}

/// 组装通知 (title, body)。
///
/// - title：项目名保留末两段（`…/` 前缀），长路径不挤占横幅首屏
/// - body：完成状态在最前（横幅截断也不丢核心信息），缺失段自动省略
pub(crate) fn turn_complete_notification(
    context: &TurnCompleteNotificationContext<'_>,
) -> (String, String) {
    let title = match non_empty(context.path_base) {
        Some(path_base) => format!("aemeath · {}", shorten_path_base(path_base)),
        None => "aemeath".to_string(),
    };

    let completion = match context.duration_ms {
        Some(millis) => format!(
            "Turn complete in {}",
            crate::tui::model::conversation::terminal::format_duration(
                std::time::Duration::from_millis(millis)
            )
        ),
        None => "Turn complete".to_string(),
    };

    let mut body_segments: Vec<&str> = vec![&completion];
    if let Some(branch) = non_empty(context.branch) {
        body_segments.push(branch);
    }
    if let Some(prompt) = non_empty(context.prompt) {
        body_segments.push(prompt);
    }

    (title, body_segments.join(" · "))
}

/// 长路径只保留末两段并加 `…/` 前缀；两段以内原样返回。
///
/// `/` 与 `\` 都作为分隔符（兼容 Windows 风格路径），统一以 `/` 重新拼接。
fn shorten_path_base(path_base: &str) -> String {
    let segments: Vec<&str> = path_base
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty())
        .collect();
    match segments.len() {
        0..=2 => path_base.to_string(),
        _ => format!("…/{}", segments[segments.len() - 2..].join("/")),
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// 格式化 OSC 777 通知序列。
///
/// title 字段以 `;` 分隔：title 内的 `;` 替换为 `:`，否则正文被吞进标题；
/// 两个字段内的控制字符（BEL/ESC/换行等）一律删除，防止序列被截断或注入。
pub(crate) fn format_osc777_notification(title: &str, body: &str) -> String {
    format!(
        "\x1b]777;notify;{};{}\x07",
        sanitize_field(title, true),
        sanitize_field(body, false)
    )
}

/// 将通知序列写入终端 writer（生产入口为 `stdout`）。
pub(crate) fn write_terminal_notification(
    writer: &mut impl io::Write,
    title: &str,
    body: &str,
) -> io::Result<()> {
    writer.write_all(format_osc777_notification(title, body).as_bytes())?;
    writer.flush()
}

/// 清洗单个字段：删除控制字符；title 字段额外把 `;` 替换为 `:`。
fn sanitize_field(text: &str, is_title: bool) -> String {
    text.chars()
        .filter(|ch| !ch.is_control())
        .map(|ch| if is_title && ch == ';' { ':' } else { ch })
        .collect()
}
