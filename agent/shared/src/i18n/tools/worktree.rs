//! Worktree 工具文案（EnterWorktree / ExitWorktree 的 description、guidance、error）。
//!
//! guidance 明确区分 path_base（相对路径解析基）与 working_root（安全边界）语义（#413）。
//! ExitWorktree guidance 对称（#415）。

use std::path::Path;

/// EnterWorktree description。
pub fn enter_description(lang: &str) -> &'static str {
    match lang {
        "zh" => "进入或创建 git worktree 目录，将当前工作上下文压栈保存。path 可选：省略时从 branch 推导为 .worktrees/<安全分支名>，其中路径分隔符和敏感字符会替换为 -。如果目标路径不存在，本工具会自动基于 main 执行 git worktree add 创建 worktree 后再进入。开 worktree 时必须调用本工具，NEVER 在主 checkout 中用 git checkout -b 或 git switch -c 代替 worktree。使用场景：当需要在不同分支的 worktree 中工作时，可以切换到目标 worktree 进行文件读取、编辑、执行命令等操作，完成后通过 ExitWorktree 恢复原始上下文。注意：不允许嵌套进入，必须先 ExitWorktree 退出当前 worktree 才能进入新的。已进入非 main 分支不代表已在 worktree；以本工具返回的 path_base/working_root 为准。",
        _ => "Enter or create a git worktree directory, pushing the current working context onto a stack. path is optional: when omitted it is derived from branch as .worktrees/<safe-branch-name>, with path separators and sensitive characters replaced by -. If the target path does not exist, this tool runs `git worktree add` based on main to create it before entering. You MUST call this tool to open a worktree; NEVER use `git checkout -b` or `git switch -c` in the main checkout instead. Use case: when you need to work in a worktree on a different branch, you can switch to the target worktree to read files, edit, and run commands, then restore the original context via ExitWorktree when done. Note: nested entry is not allowed; you must ExitWorktree the current one before entering a new one. Being on a non-main branch does not mean you are in a worktree; trust the path_base/working_root returned by this tool.",
    }
}

/// ExitWorktree description。
pub fn exit_description(lang: &str) -> &'static str {
    match lang {
        "zh" => "退出当前 worktree，恢复进入前的上下文（从上下文栈中弹出）。如果提供了 path 参数，则直接切换到指定路径（等效于 EnterWorktree 后立即 pop 栈顶）。如果没有提供 path 参数，则恢复上一次 EnterWorktree 保存的工作目录。当上下文栈为空时返回错误。",
        _ => "Exit the current worktree, restoring the context from before entry (popping the context stack). If a path argument is provided, switch directly to that path (equivalent to EnterWorktree followed by an immediate stack pop). If no path argument is provided, restore the working directory saved by the last EnterWorktree. Returns an error when the context stack is empty.",
    }
}

/// 进入 worktree 后的 guidance（#413：明确 path_base/working_root 语义）。
///
/// - path_base = 相对路径解析基（LLM 传相对路径时按此拼绝对路径）
/// - working_root = 安全边界（绝对路径必须位于其下）
pub fn enter_guidance(lang: &str) -> &'static str {
    match lang {
        "zh" => "已切换工作区上下文。后续 Read/Edit/Write/Glob/Grep/Bash 请优先使用相对路径，系统会以返回的 path_base 为解析基拼成绝对路径。如必须使用绝对路径，该路径必须位于 working_root 之内（安全边界），否则会被拒绝。切勿继续使用进入 worktree 前的 checkout/main workspace 绝对路径。",
        _ => "Workspace context switched. For subsequent Read/Edit/Write/Glob/Grep/Bash calls, prefer relative paths — the system resolves them against the returned path_base to form absolute paths. If an absolute path is unavoidable, it MUST fall inside working_root (the safety boundary) or it will be rejected. Do not keep using absolute paths from the checkout/main workspace you were in before entering the worktree.",
    }
}

/// 退出 worktree（恢复上一上下文）后的 guidance（#415 对称）。
///
/// `restored_to` 为恢复后的 path_base 显示文本。
pub fn exit_guidance(lang: &str, restored_to: &Path) -> String {
    match lang {
        "zh" => format!(
            "已退出 worktree，恢复到 {restored_to}。后续路径以当前 path_base（相对路径解析基）为准；绝对路径必须位于当前 working_root（安全边界）之内。切勿继续使用刚退出的 worktree 内的绝对路径。",
            restored_to = restored_to.display()
        ),
        _ => format!(
            "Exited worktree, restored to {restored_to}. Subsequent paths follow the current path_base (relative-path resolution base); absolute paths MUST fall inside the current working_root (safety boundary). Do not keep using absolute paths from the worktree you just exited.",
            restored_to = restored_to.display()
        ),
    }
}

/// switch_to（直接切换路径）后的 guidance（#415 对称）。
///
/// `switched_to` 为切换目标的显示文本。
pub fn switch_guidance(lang: &str, switched_to: &str) -> String {
    match lang {
        "zh" => format!(
            "已切换到 {switched_to}。后续路径以当前 path_base（相对路径解析基）为准；绝对路径必须位于当前 working_root（安全边界）之内。"
        ),
        _ => format!(
            "Switched to {switched_to}. Subsequent paths follow the current path_base (relative-path resolution base); absolute paths MUST fall inside the current working_root (safety boundary)."
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

/// 切换路径失败。
pub fn switch_error(lang: &str, detail: impl std::fmt::Display) -> String {
    match lang {
        "zh" => format!("切换路径失败：{detail}"),
        _ => format!("Failed to switch path: {detail}"),
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
mod tests {
    use super::*;

    #[test]
    fn descriptions_are_bilingual_and_fallback_en() {
        assert!(enter_description("zh").contains("进入"));
        assert!(enter_description("en").contains("Enter"));
        assert_eq!(enter_description("fr"), enter_description("en"));
        assert!(exit_description("zh").contains("退出"));
        assert!(exit_description("en").contains("Exit"));
        assert_eq!(exit_description("xx"), exit_description("en"));
    }

    #[test]
    fn enter_guidance_distinguishes_path_base_and_working_root() {
        let zh = enter_guidance("zh");
        let en = enter_guidance("en");
        for s in [&zh, &en] {
            assert!(s.contains("path_base"), "guidance must mention path_base");
            assert!(
                s.contains("working_root"),
                "guidance must mention working_root"
            );
        }
        assert_eq!(enter_guidance("fr"), en);
    }

    #[test]
    fn exit_guidance_includes_restored_target() {
        let g = exit_guidance("en", std::path::Path::new("/tmp/foo"));
        assert!(g.contains("/tmp/foo"));
        assert!(g.contains("path_base"));
        let zh = exit_guidance("zh", std::path::Path::new("/tmp/foo"));
        assert!(zh.contains("/tmp/foo"));
        assert!(zh.contains("path_base"));
    }

    #[test]
    fn switch_guidance_includes_target() {
        let g = switch_guidance("en", "/some/path");
        assert!(g.contains("/some/path"));
        assert!(g.contains("path_base"));
    }

    #[test]
    fn errors_are_bilingual() {
        assert!(enter_error("zh", "x").contains("失败"));
        assert!(enter_error("en", "x").contains("Failed"));
    }
}
