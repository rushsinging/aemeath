//! Worktree 工具文案（EnterWorktree / ExitWorktree 的 description、guidance、error）。
//!
//! guidance 明确区分 path_base（相对路径解析基）与 workspace_root（安全边界）语义（#413）。
//! ExitWorktree guidance 对称（#415）。

use std::path::Path;

/// EnterWorktree description。
pub fn enter_description(lang: &str) -> &'static str {
    match lang {
        "zh" => "进入或创建 git worktree，压栈保存当前上下文，用 ExitWorktree 恢复。需要在其他分支上工作时使用。不允许嵌套进入——须先退出当前 worktree。",
        _ => "Enter or create a git worktree, pushing the current context; restore it with ExitWorktree. Use when work must happen on a different branch. Nested entry is rejected — exit the current worktree first.",
    }
}

/// ExitWorktree description。
pub fn exit_description(lang: &str) -> &'static str {
    match lang {
        "zh" => "退出当前 worktree，恢复最近一次 EnterWorktree 保存的上下文。本工具不接受参数；上下文栈为空时返回错误。",
        _ => "Exit the current worktree and restore the context saved by the most recent EnterWorktree. This tool takes no arguments and errors when no worktree context is on the stack.",
    }
}

/// 进入 worktree 后的 guidance（#413：明确 path_base/workspace_root 语义）。
///
/// - path_base = 相对路径解析基（LLM 传相对路径时按此拼绝对路径）
/// - workspace_root = 安全边界（绝对路径必须位于其下）
pub fn enter_guidance(lang: &str) -> &'static str {
    match lang {
        "zh" => "已切换工作区上下文。后续 Read/Edit/Write/Glob/Grep/Bash 请优先使用相对路径，系统会以返回的 path_base 为解析基拼成绝对路径。如必须使用绝对路径，该路径必须位于 workspace_root 之内（安全边界），否则会被拒绝。切勿继续使用进入 worktree 前的 checkout/main workspace 绝对路径。",
        _ => "Workspace context switched. For subsequent Read/Edit/Write/Glob/Grep/Bash calls, prefer relative paths — the system resolves them against the returned path_base to form absolute paths. If an absolute path is unavoidable, it MUST fall inside workspace_root (the safety boundary) or it will be rejected. Do not keep using absolute paths from the checkout/main workspace you were in before entering the worktree.",
    }
}

/// 退出 worktree（恢复上一上下文）后的 guidance（#415 对称）。
///
/// `restored_to` 为恢复后的 path_base 显示文本。
pub fn exit_guidance(lang: &str, restored_to: &Path) -> String {
    match lang {
        "zh" => format!(
            "已退出 worktree，恢复到 {restored_to}。后续路径以当前 path_base（相对路径解析基）为准；绝对路径必须位于当前 workspace_root（安全边界）之内。切勿继续使用刚退出的 worktree 内的绝对路径。",
            restored_to = restored_to.display()
        ),
        _ => format!(
            "Exited worktree, restored to {restored_to}. Subsequent paths follow the current path_base (relative-path resolution base); absolute paths MUST fall inside the current workspace_root (safety boundary). Do not keep using absolute paths from the worktree you just exited.",
            restored_to = restored_to.display()
        ),
    }
}

/// 进入 worktree 失败。
pub fn enter_error(lang: &str, detail: impl std::fmt::Display) -> String {
    match lang {
        "zh" => format!("进入 worktree 失败：{detail}"),
        _ => format!("Failed to enter worktree: {detail}"),
    }
}

/// 退出 worktree 失败。
pub fn exit_error(lang: &str, detail: impl std::fmt::Display) -> String {
    match lang {
        "zh" => format!("退出 worktree 失败：{detail}"),
        _ => format!("Failed to exit worktree: {detail}"),
    }
}

/// 输入解析失败（通用）。
pub fn invalid_input_error(lang: &str, detail: impl std::fmt::Display) -> String {
    match lang {
        "zh" => format!("输入无效：{detail}"),
        _ => format!("Invalid input: {detail}"),
    }
}

#[cfg(test)]
#[path = "worktree_tests.rs"]
mod tests;
