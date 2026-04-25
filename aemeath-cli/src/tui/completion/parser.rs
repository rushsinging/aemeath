//! 输入解析与补全 token 提取

use super::types::TriggerType;

/// 提取光标位置处的补全 token
/// 如果找到触发器，返回 (token, 起始位置, 触发类型)
pub fn extract_completion_token(input: &str, cursor_offset: usize) -> Option<(String, usize, TriggerType)> {
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

    let before_cursor = &input[..cursor_offset];

    // 检查 /model <arg> 触发器（模型名称补全）
    if input.starts_with("/model ") && cursor_offset >= 7 {
        let arg_start = 7; // "/model " 的长度
        let after_cmd = &input[arg_start..];
        // 不为 "/model list" 或 "/model list ..." 触发
        if after_cmd.starts_with("list") && (after_cmd.len() == 4 || after_cmd.as_bytes()[4] == b' ') {
            // 继续检查其他触发器
        } else {
            let arg_part = &input[arg_start..cursor_offset];
            // 包含光标后直到空格的文本用于匹配
            let after_cursor = &input[cursor_offset..];
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
        if input.len() > 6 && !input.chars().nth(6).map_or(false, |c| c.is_whitespace()) {
            // 仍在输入 "/model..." 命令
        } else {
            // "/model " 后可能有子命令
            let _after_model = if input.len() > 6 {
                &input[6..] // 跳过 "/model"
            } else {
                ""
            };
            // 如果看起来像子命令（不是完整模型名），则返回 ModelSubCommand
            let arg_part = &input[7..cursor_offset].trim_start(); // 跳过 "/model "
            if !arg_part.is_empty() {
                return Some((arg_part.to_string(), 7, TriggerType::ModelSubCommand));
            }
        }
    }

    // 检查 @ 触发器（文件/路径补全）
    if let Some(at_pos) = before_cursor.rfind('@') {
        let is_start_or_after_space = at_pos == 0
            || before_cursor[..at_pos].ends_with(char::is_whitespace);
        if is_start_or_after_space {
            let after_at = &before_cursor[at_pos + 1..]; // '@' 是 ASCII，+1 安全
            let after_cursor = &input[cursor_offset..];
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
        let end = input.find(' ').unwrap_or(input.len()).min(cursor_offset);
        let token = &before_cursor[..end];
        return Some((token.to_string(), 0, TriggerType::SlashCommand));
    }

    // 检查输入中间的斜杠命令（空格后跟 /）
    if let Some(space_slash_pos) = before_cursor.rfind(" /") {
        let slash_pos = space_slash_pos + 1; // ' ' 是 ASCII，+1 安全
        let after_slash = &before_cursor[slash_pos + 1..]; // '/' 是 ASCII，+1 安全
        if !after_slash.contains(' ') {
            let token = format!("/{}", after_slash);
            return Some((token, slash_pos, TriggerType::SlashCommand));
        }
    }

    None
}
