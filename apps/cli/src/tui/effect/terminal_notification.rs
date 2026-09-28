//! OSC 777（RXVT notify）终端桌面通知：序列格式化与写出。
//!
//! 回合完成时向终端写 `\x1b]777;notify;<title>;<body>\x07`，由终端自身决定
//! 是否弹桌面提醒（cmux/iTerm2 在窗口聚焦时自行抑制）；对不支持 OSC 777 的
//! 终端该序列无害，**NEVER** 做终端能力猜测。

use std::io;

#[cfg(test)]
#[path = "terminal_notification_tests.rs"]
mod tests;

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

/// 回合完成通知正文：带耗时时显示耗时，否则为纯完成提示。
pub(crate) fn turn_complete_notification_body(duration_ms: Option<u64>) -> String {
    match duration_ms {
        Some(millis) => format!(
            "Turn complete in {}",
            crate::tui::model::conversation::terminal::format_duration(
                std::time::Duration::from_millis(millis)
            )
        ),
        None => "Turn complete".to_string(),
    }
}

/// 清洗单个字段：删除控制字符；title 字段额外把 `;` 替换为 `:`。
fn sanitize_field(text: &str, is_title: bool) -> String {
    text.chars()
        .filter(|ch| !ch.is_control())
        .map(|ch| if is_title && ch == ';' { ':' } else { ch })
        .collect()
}
