use std::path::{Path, PathBuf};

/// 保存进入 worktree 前的工作上下文快照
#[derive(Debug, Clone)]
pub struct WorkingContext {
    pub path_base: PathBuf,
    pub working_root: PathBuf,
}

/// 检查两个路径是否属于同一 git 仓库
pub fn is_same_git_repo(a: &Path, b: &Path) -> Result<bool, String> {
    let git_dir_a = get_git_dir(a)?;
    let git_dir_b = get_git_dir(b)?;
    Ok(git_dir_a == git_dir_b)
}

pub fn get_git_dir(path: &Path) -> Result<PathBuf, String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(path)
        .output()
        .map_err(|e| format!("git rev-parse --git-dir 执行失败: {}", e))?;

    if !output.status.success() {
        return Err("无法获取 git dir".to_string());
    }

    let git_dir_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let git_dir = PathBuf::from(&git_dir_str);
    if git_dir.is_absolute() {
        Ok(git_dir)
    } else {
        Ok(path.join(&git_dir_str).canonicalize().unwrap_or_else(|_| path.join(&git_dir_str)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolContext;
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};
    use tokio_util::sync::CancellationToken;

    fn new_test_context() -> ToolContext {
        let cwd = std::env::current_dir().unwrap();
        let (_, working_root, path_base) = ToolContext::new_working_paths(cwd);
        ToolContext {
            cwd: PathBuf::from("/tmp/test"),
            working_root,
            path_base,
            cancel: CancellationToken::new(),
            read_files: Arc::new(Mutex::new(HashSet::new())),
            agent_runner: None,
            session_reminders: None,
            memory_config: crate::config::MemoryConfig::default(),
            plan_mode: None,
            allow_all: false,
            max_tool_concurrency: 4,
            max_agent_concurrency: 2,
            agent_semaphore: Arc::new(tokio::sync::Semaphore::new(2)),
            progress_tx: None,
            parent_session_id: None,
            context_stack: Arc::new(Mutex::new(Vec::new())),
        }
    }

    #[test]
    fn test_context_stack_push_pop() {
        let ctx = new_test_context();

        // exit_worktree on empty stack should error
        assert!(ctx.exit_worktree().is_err());

        // 模拟 push/pop
        ctx.context_stack
            .lock()
            .unwrap()
            .push(WorkingContext {
                path_base: PathBuf::from("/tmp/prev"),
                working_root: PathBuf::from("/tmp/prev"),
            });
        let result = ctx.exit_worktree().unwrap();
        assert_eq!(result.path_base, PathBuf::from("/tmp/prev"));
        assert_eq!(ctx.current_path_base(), PathBuf::from("/tmp/prev"));
    }

    #[test]
    fn test_enter_worktree_rejects_nonexistent_path() {
        let ctx = new_test_context();
        assert!(ctx.enter_worktree(PathBuf::from("/nonexistent/path")).is_err());
    }
}
