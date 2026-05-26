use std::path::{Path, PathBuf};

use crate::session_types::{WorkspaceContext, WorkspaceStackEntry};
use crate::tool::{ToolContext, WorkingContext};

/// 检查两个路径是否属于同一 git 仓库
pub fn is_same_git_repo(a: &Path, b: &Path) -> Result<bool, String> {
    let git_common_dir_a = get_git_common_dir(a)?;
    let git_common_dir_b = get_git_common_dir(b)?;
    Ok(git_common_dir_a == git_common_dir_b)
}

pub fn get_git_common_dir(path: &Path) -> Result<PathBuf, String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--git-common-dir"])
        .current_dir(path)
        .output()
        .map_err(|e| format!("git rev-parse --git-common-dir 执行失败: {}", e))?;

    if !output.status.success() {
        return Err("无法获取 git common dir".to_string());
    }

    let git_common_dir_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let git_common_dir = PathBuf::from(&git_common_dir_str);
    if git_common_dir.is_absolute() {
        Ok(git_common_dir.canonicalize().unwrap_or(git_common_dir))
    } else {
        Ok(path
            .join(&git_common_dir_str)
            .canonicalize()
            .unwrap_or_else(|_| path.join(&git_common_dir_str)))
    }
}

/// 进入指定 worktree：push 当前上下文，然后切换 path_base/working_root
pub fn enter_worktree(ctx: &ToolContext, path: PathBuf) -> Result<WorkingContext, String> {
    // 拒绝嵌套：必须先 ExitWorktree 再 EnterWorktree
    {
        let stack = ctx.context_stack.lock().unwrap_or_else(|e| e.into_inner());
        if !stack.is_empty() {
            return Err(
                "已在 worktree 中，请先 ExitWorktree 退出当前 worktree 再进入新的".to_string(),
            );
        }
    }

    let path = if !path.is_absolute() {
        ctx.current_path_base().join(path)
    } else {
        path
    };

    // 校验路径存在且是 git worktree
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("路径不存在或无法访问 {}: {}", path.display(), e))?;

    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&canonical)
        .output()
        .map_err(|e| format!("git rev-parse 执行失败: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "路径 {} 不是 git 仓库或 worktree",
            canonical.display()
        ));
    }

    let worktree_root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let worktree_root = PathBuf::from(&worktree_root);

    // 校验是否与当前 repo 同源（同一 git common dir）
    let current_root = ctx.current_working_root();
    if let Ok(same) = is_same_git_repo(&current_root, &worktree_root) {
        if !same {
            return Err(format!(
                "路径 {} 不属于当前仓库（当前仓库根: {}）",
                worktree_root.display(),
                current_root.display()
            ));
        }
    }

    // 保存当前上下文
    let snapshot = WorkingContext {
        path_base: ctx.current_path_base(),
        working_root: ctx.current_working_root(),
    };
    ctx.context_stack
        .lock()
        .map(|mut s| s.push(snapshot.clone()))
        .unwrap_or_else(|e| e.into_inner().push(snapshot.clone()));

    // 切换到新 worktree
    ctx.set_working_directory(canonical);

    Ok(snapshot)
}

/// 退出当前 worktree：pop 栈恢复之前的上下文
pub fn exit_worktree(ctx: &ToolContext) -> Result<WorkingContext, String> {
    let mut stack = ctx.context_stack.lock().unwrap_or_else(|e| e.into_inner());

    match stack.pop() {
        Some(prev) => {
            match ctx.working_root.lock() {
                Ok(mut wr) => *wr = prev.working_root.clone(),
                Err(poisoned) => *poisoned.into_inner() = prev.working_root.clone(),
            }
            match ctx.path_base.lock() {
                Ok(mut pb) => *pb = prev.path_base.clone(),
                Err(poisoned) => *poisoned.into_inner() = prev.path_base.clone(),
            }
            Ok(prev)
        }
        None => Err("上下文栈为空，没有可恢复的 worktree。可能已经在主工作区。".to_string()),
    }
}

/// 将当前 ToolContext 的工作上下文转换为可持久化的会话工作区上下文。
pub fn workspace_context_from_tool_context(ctx: &ToolContext) -> WorkspaceContext {
    let stack = ctx
        .context_stack
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|entry| WorkspaceStackEntry {
            path_base: entry.path_base.display().to_string(),
            working_root: entry.working_root.display().to_string(),
        })
        .collect();

    WorkspaceContext {
        path_base: ctx.current_path_base().display().to_string(),
        working_root: ctx.current_working_root().display().to_string(),
        context_stack: stack,
    }
}

/// 从会话工作区上下文恢复 ToolContext 的 path_base/working_root/context_stack。
pub fn restore_workspace_context(
    ctx: &ToolContext,
    workspace: &WorkspaceContext,
) -> Result<(), String> {
    let path_base = PathBuf::from(&workspace.path_base);
    let working_root = PathBuf::from(&workspace.working_root);

    if !path_base.exists() {
        return Err(format!(
            "恢复工作目录失败：路径不存在 {}",
            path_base.display()
        ));
    }
    if !working_root.exists() {
        return Err(format!(
            "恢复仓库根目录失败：路径不存在 {}",
            working_root.display()
        ));
    }

    let stack = workspace
        .context_stack
        .iter()
        .map(|entry| WorkingContext {
            path_base: PathBuf::from(&entry.path_base),
            working_root: PathBuf::from(&entry.working_root),
        })
        .collect::<Vec<_>>();

    match ctx.working_root.lock() {
        Ok(mut wr) => *wr = working_root,
        Err(poisoned) => *poisoned.into_inner() = working_root,
    }
    match ctx.path_base.lock() {
        Ok(mut pb) => *pb = path_base,
        Err(poisoned) => *poisoned.into_inner() = path_base,
    }
    match ctx.context_stack.lock() {
        Ok(mut current_stack) => *current_stack = stack,
        Err(poisoned) => *poisoned.into_inner() = stack,
    }

    Ok(())
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
        assert!(exit_worktree(&ctx).is_err());

        // 模拟 push/pop
        ctx.context_stack.lock().unwrap().push(WorkingContext {
            path_base: PathBuf::from("/tmp/prev"),
            working_root: PathBuf::from("/tmp/prev"),
        });
        let result = exit_worktree(&ctx).unwrap();
        assert_eq!(result.path_base, PathBuf::from("/tmp/prev"));
        assert_eq!(ctx.current_path_base(), PathBuf::from("/tmp/prev"));
    }

    #[test]
    fn test_enter_worktree_rejects_nonexistent_path() {
        let ctx = new_test_context();
        assert!(enter_worktree(&ctx, PathBuf::from("/nonexistent/path")).is_err());
    }

    #[test]
    fn test_enter_worktree_rejects_nested_enter() {
        let ctx = new_test_context();
        // 模拟已在 worktree 中（栈非空）
        ctx.context_stack.lock().unwrap().push(WorkingContext {
            path_base: PathBuf::from("/tmp/prev"),
            working_root: PathBuf::from("/tmp/prev"),
        });
        let result = enter_worktree(&ctx, PathBuf::from("/tmp/another"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("先 ExitWorktree"));
    }

    #[test]
    fn test_is_same_git_repo_accepts_linked_worktree() {
        let repo_root = std::env::current_dir().unwrap();
        let worktree_root = std::env::temp_dir().join(format!(
            "aemeath_worktree_same_repo_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let add_output = std::process::Command::new("git")
            .args([
                "worktree",
                "add",
                "--detach",
                worktree_root.to_str().unwrap(),
                "HEAD",
            ])
            .current_dir(&repo_root)
            .output()
            .unwrap();
        assert!(
            add_output.status.success(),
            "git worktree add failed: {}",
            String::from_utf8_lossy(&add_output.stderr)
        );

        let result = is_same_git_repo(&repo_root, &worktree_root);

        let _ = std::process::Command::new("git")
            .args([
                "worktree",
                "remove",
                "--force",
                worktree_root.to_str().unwrap(),
            ])
            .current_dir(&repo_root)
            .output();
        assert!(result.unwrap());
    }
}
