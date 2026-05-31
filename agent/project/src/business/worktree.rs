use std::path::{Path, PathBuf};

use share::session_types::{WorkspaceContext, WorkspaceStackEntry};
use share::tool::{ToolContext, WorkingContext};

use super::working_paths::{current_path, set_working_directory};

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

/// 判断指定路径是否位于 git worktree 中（而非主 checkout）。
/// 在 worktree 中时，`git rev-parse --git-dir` 会返回包含 `.git/worktrees/` 的路径。
fn in_worktree(path: &Path) -> bool {
    std::process::Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(path)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let git_dir = String::from_utf8_lossy(&o.stdout).trim().to_string();
                Some(git_dir.contains("/.git/worktrees/"))
            } else {
                None
            }
        })
        .unwrap_or(false)
}

const DEFAULT_WORKTREE_BASE: &str = "main";
const DEFAULT_WORKTREE_DIR: &str = ".worktrees";

fn sanitize_branch_for_path(branch: &str) -> Result<String, String> {
    let mut sanitized = String::new();
    let mut last_was_dash = false;

    for ch in branch.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            sanitized.push(ch);
            last_was_dash = false;
        } else if !last_was_dash {
            sanitized.push('-');
            last_was_dash = true;
        }
    }

    let sanitized = sanitized
        .trim_matches(|ch| matches!(ch, '.' | '_' | '-'))
        .to_string();
    if sanitized.is_empty() {
        return Err("branch 不能只包含路径分隔符或敏感字符".to_string());
    }
    Ok(sanitized)
}

fn derive_path_from_branch(ctx: &ToolContext, branch: &str) -> Result<PathBuf, String> {
    Ok(current_path(&ctx.path_base)
        .join(DEFAULT_WORKTREE_DIR)
        .join(sanitize_branch_for_path(branch)?))
}

fn resolve_worktree_path(
    ctx: &ToolContext,
    path: Option<PathBuf>,
    branch: Option<&str>,
) -> Result<PathBuf, String> {
    match path {
        Some(path) if path.is_absolute() => Ok(path),
        Some(path) => Ok(current_path(&ctx.path_base).join(path)),
        None => match branch {
            Some(branch) if !branch.trim().is_empty() => derive_path_from_branch(ctx, branch),
            _ => Err("进入或创建 worktree 时必须提供 path 或 branch".to_string()),
        },
    }
}

fn create_worktree(ctx: &ToolContext, path: &Path, branch: Option<String>) -> Result<(), String> {
    let repo_root = current_path(&ctx.working_root);
    let branch = branch
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "创建新 worktree 时必须提供 branch".to_string())?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("创建 worktree 父目录失败 {}: {}", parent.display(), e))?;
    }

    let output = std::process::Command::new("git")
        .args(["worktree", "add"])
        .arg(path)
        .args(["-b", branch.as_str(), DEFAULT_WORKTREE_BASE])
        .current_dir(&repo_root)
        .output()
        .map_err(|e| format!("git worktree add 执行失败: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "创建 worktree 失败：git worktree add {} -b {} {}\nstdout: {}\nstderr: {}",
            path.display(),
            branch,
            DEFAULT_WORKTREE_BASE,
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(())
}

/// 进入指定 worktree：目标不存在时自动创建，push 当前上下文，然后切换 path_base/working_root
pub fn enter_worktree(
    ctx: &ToolContext,
    path: Option<PathBuf>,
    branch: Option<String>,
) -> Result<WorkingContext, String> {
    // 拒绝嵌套：必须先 ExitWorktree 再 EnterWorktree。
    // 增加 git 实际状态校验，防止上下文栈残留导致误判（refs #96）。
    {
        let mut stack = ctx.context_stack.lock().unwrap_or_else(|e| e.into_inner());
        if !stack.is_empty() {
            // 二次校验：git 层面是否真的在 worktree 中
            let current = current_path(&ctx.path_base);
            if !in_worktree(&current) {
                // 上下文栈残留（如上次会话异常结束未清理），自动清除
                stack.clear();
            } else {
                return Err(
                    "已在 worktree 中，请先 ExitWorktree 退出当前 worktree 再进入新的".to_string(),
                );
            }
        }
    }

    let path = resolve_worktree_path(ctx, path, branch.as_deref())?;

    if !path.exists() {
        create_worktree(ctx, &path, branch)?;
    }

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
    let current_root = current_path(&ctx.working_root);
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
        path_base: current_path(&ctx.path_base),
        working_root: current_path(&ctx.working_root),
    };
    ctx.context_stack
        .lock()
        .map(|mut s| s.push(snapshot.clone()))
        .unwrap_or_else(|e| e.into_inner().push(snapshot.clone()));

    // 切换到新 worktree
    set_working_directory(&ctx.working_root, &ctx.path_base, canonical);

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
        path_base: current_path(&ctx.path_base).display().to_string(),
        working_root: current_path(&ctx.working_root).display().to_string(),
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
    use share::tool::ToolContext;
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};
    use tokio_util::sync::CancellationToken;

    fn new_test_context() -> ToolContext {
        let cwd = std::env::current_dir().unwrap();
        let (_, working_root, path_base) = crate::api::new_working_paths(cwd);
        ToolContext {
            cwd: PathBuf::from("/tmp/test"),
            working_root,
            path_base,
            cancel: CancellationToken::new(),
            read_files: Arc::new(Mutex::new(HashSet::new())),
            agent_runner: None,
            session_reminders: None,
            memory_config: share::config::MemoryConfig::default(),
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
        assert_eq!(current_path(&ctx.path_base), PathBuf::from("/tmp/prev"));
    }

    #[test]
    fn test_sanitize_branch_for_path_replaces_sensitive_chars() {
        let sanitized = sanitize_branch_for_path("feature/refs-47:P15 runtime").unwrap();

        assert_eq!(sanitized, "feature-refs-47-P15-runtime");
    }

    #[test]
    fn test_sanitize_branch_for_path_rejects_empty_result() {
        let result = sanitize_branch_for_path("///:::   ");

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("敏感字符"));
    }

    #[test]
    fn test_enter_worktree_derives_path_from_branch() {
        let ctx = new_test_context();
        let repo_root = current_path(&ctx.working_root);
        let branch = format!(
            "test/derive-path-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let expected_path = derive_path_from_branch(&ctx, &branch).unwrap();

        let result = enter_worktree(&ctx, None, Some(branch.clone()));
        assert!(result.is_ok(), "{}", result.unwrap_err());
        let expected_path_base = expected_path.canonicalize().unwrap();
        let actual_path_base = current_path(&ctx.path_base);

        let _ = exit_worktree(&ctx);
        let _ = std::process::Command::new("git")
            .args([
                "worktree",
                "remove",
                "--force",
                expected_path.to_str().unwrap(),
            ])
            .current_dir(&repo_root)
            .output();
        let _ = std::process::Command::new("git")
            .args(["branch", "-D", branch.as_str()])
            .current_dir(&repo_root)
            .output();

        assert_eq!(actual_path_base, expected_path_base);
        assert!(actual_path_base.ends_with(sanitize_branch_for_path(&branch).unwrap()));
    }

    #[test]
    fn test_enter_worktree_rejects_missing_path_and_branch() {
        let ctx = new_test_context();

        let result = enter_worktree(&ctx, None, None);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("path 或 branch"));
    }

    #[test]
    fn test_enter_worktree_auto_clears_stale_stack_not_in_worktree() {
        let ctx = new_test_context();
        let main_checkout = get_git_common_dir(&std::env::current_dir().unwrap())
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        set_working_directory(&ctx.working_root, &ctx.path_base, main_checkout);
        // 模拟上下文栈残留（栈非空但 git 不在 worktree 中）
        ctx.context_stack.lock().unwrap().push(WorkingContext {
            path_base: PathBuf::from("/tmp/stale"),
            working_root: PathBuf::from("/tmp/stale"),
        });
        // 不应因残留栈而拒绝，应自动清理后继续（refs #96）
        // 这里路径不存在且无 branch，会走到 create_worktree 的错误路径
        let result = enter_worktree(&ctx, Some(PathBuf::from("/tmp/nonexistent")), None);
        assert!(result.is_err());
        let error = result.unwrap_err();
        // 不应报"先 ExitWorktree"，报错应为路径/branch 相关
        assert!(!error.contains("先 ExitWorktree"), "{error}");
        assert!(error.contains("branch"), "{error}");
    }

    /// 测试在 worktree 中嵌套 enter 被拒绝。此测试需要在 worktree 中运行才有效，
    /// 主 checkout 中栈非空会被 in_worktree() 判定为残留并自动清理。
    #[test]
    #[ignore = "需要在 worktree 中运行才能触发嵌套拒绝逻辑"]
    fn test_enter_worktree_rejects_nested_enter_in_worktree() {
        let ctx = new_test_context();
        ctx.context_stack.lock().unwrap().push(WorkingContext {
            path_base: PathBuf::from("/tmp/prev"),
            working_root: PathBuf::from("/tmp/prev"),
        });
        let result = enter_worktree(&ctx, Some(PathBuf::from("/tmp/another")), None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("先 ExitWorktree"));
    }

    #[test]
    fn test_enter_worktree_creates_missing_path_with_branch() {
        let ctx = new_test_context();
        let repo_root = current_path(&ctx.working_root);
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let worktree_root = std::env::temp_dir().join(format!("aemeath_auto_worktree_{suffix}"));
        let branch = format!("test/aemeath-auto-worktree-{suffix}");

        let result = enter_worktree(&ctx, Some(worktree_root.clone()), Some(branch.clone()));
        let expected_path_base = worktree_root.canonicalize().unwrap();
        let actual_path_base = current_path(&ctx.path_base);

        let _ = exit_worktree(&ctx);
        let _ = std::process::Command::new("git")
            .args([
                "worktree",
                "remove",
                "--force",
                worktree_root.to_str().unwrap(),
            ])
            .current_dir(&repo_root)
            .output();
        let _ = std::process::Command::new("git")
            .args(["branch", "-D", branch.as_str()])
            .current_dir(&repo_root)
            .output();

        assert!(result.is_ok(), "{}", result.unwrap_err());
        assert_eq!(actual_path_base, expected_path_base);
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
