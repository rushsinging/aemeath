use std::io;
use std::path::Path;

/// 工作目录缺失时的中文归因文案（纯路径检查，不依赖错误对象）。
///
/// 当且仅当 `cwd` 在文件系统上不存在时返回 `Some`，文案包含路径、
/// 可能原因与建议动作，供 spawn 失败归因与 prompt/TUI 降级提示复用。
///
/// `subject` 是依赖该目录的业务主体（如「Bash 命令」「hook 命令」「git 上下文」）。
pub fn describe_cwd_gone(cwd: &Path, subject: &str) -> Option<String> {
    if cwd.exists() {
        return None;
    }
    Some(format!(
        "工作目录已不存在：{display}——{subject}不可用。\n\
         可能原因：该 worktree 在会话期间被合并后清理，或目录被外部删除。\n\
         建议：告知用户重建该 worktree 或在有效目录重启会话；在此之前所有依赖子进程的工具（Bash、Grep、hook、git）均不可用。",
        display = cwd.display()
    ))
}

/// 子进程 spawn/wait 因工作目录缺失而失败时的中文归因文案。
///
/// 当且仅当同时满足以下条件时返回 `Some`：
/// - 错误种类为 [`io::ErrorKind::NotFound`]（目录缺失时 `spawn` 的确定性表现）；
/// - `cwd` 在文件系统上确实不存在（排除可执行文件缺失等其他 NotFound 来源）。
///
/// 其余情况返回 `None`，调用方应保留原始错误信息，避免误归因。
pub fn describe_cwd_gone_failure(error: &io::Error, cwd: &Path, subject: &str) -> Option<String> {
    if error.kind() != io::ErrorKind::NotFound {
        return None;
    }
    describe_cwd_gone(cwd, subject)
}
