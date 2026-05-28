//! 输入解析与补全 token 提取

use super::types::TriggerType;
use crate::tui::render::display::safe_text;

/// 提取光标位置处的补全 token
/// 如果找到触发器，返回 (token, 起始位置, 触发类型)
pub fn extract_completion_token(
    input: &str,
    cursor_offset: usize,
) -> Option<(String, usize, TriggerType)> {
    if input.is_empty() || cursor_offset == 0 {
        return None;
    }

    // 确保 cursor_offset 在有效的字符边界
    let cursor_offset = if cursor_offset >= input.len() {
        input.len()
    } else if input.is_char_boundary(cursor_offset) {
        cursor_offset
    } else {
        // 找到 cursor_offset 之前最近的有效字符边界
        let mut pos = cursor_offset;
        while pos > 0 && !input.is_char_boundary(pos) {
            pos -= 1;
        }
        pos
    };

    if cursor_offset == 0 {
        return None;
    }

    let before_cursor = input.get(..cursor_offset).unwrap_or("");

    // 检查 /resume <arg> 触发器（session 补全）
    if input.starts_with("/resume ") && cursor_offset >= 8 {
        let arg_start = 8; // "/resume " 的长度
        let arg_part = input.get(arg_start..cursor_offset).unwrap_or("");
        let after_cursor = input.get(cursor_offset..).unwrap_or("");
        let after_until_space: String = after_cursor
            .chars()
            .take_while(|c| !c.is_whitespace())
            .collect();
        let full_arg = format!("{}{}", arg_part, after_until_space);
        return Some((full_arg, arg_start, TriggerType::ResumeArg));
    }

    // 检查 /model <arg> 触发器（模型名称补全）
    if input.starts_with("/model ") && cursor_offset >= 7 {
        let arg_start = 7; // "/model " 的长度
        let after_cmd = input.get(arg_start..).unwrap_or("");
        // 不为 "/model list" 或 "/model list ..." 触发
        if after_cmd.starts_with("list")
            && (after_cmd.len() == 4 || after_cmd.as_bytes()[4] == b' ')
        {
            // 继续检查其他触发器
        } else {
            let arg_part = input.get(arg_start..cursor_offset).unwrap_or("");
            // 包含光标后直到空格的文本用于匹配
            let after_cursor = input.get(cursor_offset..).unwrap_or("");
            let after_until_space: String = after_cursor
                .chars()
                .take_while(|c| !c.is_whitespace())
                .collect();
            let full_arg = format!("{}{}", arg_part, after_until_space);
            return Some((full_arg, arg_start, TriggerType::ModelArg));
        }
    }

    // 检查 /model 子命令补全（例如 /model l -> /model list）
    if input.starts_with("/model") && cursor_offset > 6 {
        // 检查是否还没有空格（正在输入命令）
        if input.len() > 6 && !safe_text::safe_char_at(input, 6).is_some_and(|c| c.is_whitespace())
        {
            // 仍在输入 "/model..." 命令
        } else {
            // "/model " 后可能有子命令
            let _after_model = input.get(6..).unwrap_or("");
            // 如果看起来像子命令（不是完整模型名），则返回 ModelSubCommand
            let arg_part = input.get(7..cursor_offset).unwrap_or("").trim_start(); // 跳过 "/model "
            if !arg_part.is_empty() {
                return Some((arg_part.to_string(), 7, TriggerType::ModelSubCommand));
            }
        }
    }

    // 检查 @ 触发器（文件/路径补全）
    if let Some(at_pos) = before_cursor.rfind('@') {
        let is_start_or_after_space = at_pos == 0
            || before_cursor
                .get(..at_pos)
                .is_some_and(|prefix| prefix.ends_with(char::is_whitespace));
        if is_start_or_after_space {
            let after_at = before_cursor.get(at_pos + 1..).unwrap_or(""); // '@' 是 ASCII，+1 安全
            let after_cursor = input.get(cursor_offset..).unwrap_or("");
            // 获取光标后直到空格的文本
            let after_cursor_until_space = after_cursor
                .chars()
                .take_while(|c| !c.is_whitespace())
                .collect::<String>();
            let full_token = format!("@{}{}", after_at, after_cursor_until_space);
            return Some((full_token, at_pos, TriggerType::AtSymbol));
        }
    }

    // 检查 / 触发器（斜杠命令补全）
    if input.starts_with('/') {
        // 如果光标在第一个空格之后（参数区域），不再显示命令名补全
        if let Some(space_pos) = input.find(' ') {
            if cursor_offset > space_pos {
                return None;
            }
        }
        let end = cursor_offset.min(input.len());
        let token = before_cursor.get(..end).unwrap_or(before_cursor);
        return Some((token.to_string(), 0, TriggerType::SlashCommand));
    }

    // 检查输入中间的斜杠命令（空格后跟 /）
    if let Some(space_slash_pos) = before_cursor.rfind(" /") {
        let slash_pos = space_slash_pos + 1; // ' ' 是 ASCII，+1 安全
        let after_slash = before_cursor.get(slash_pos + 1..).unwrap_or(""); // '/' 是 ASCII，+1 安全
        if !after_slash.contains(' ') {
            let token = format!("/{}", after_slash);
            return Some((token, slash_pos, TriggerType::SlashCommand));
        }
    }

    None
}
