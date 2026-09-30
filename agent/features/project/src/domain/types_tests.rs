use super::{GitProbeError, WorkspaceError};

#[test]
fn workspace_error_display_when_git_probe_cwd_gone_shows_attribution() {
    let attribution = "工作目录已不存在：/tmp/removed-worktree——git 命令无法派生子进程。";
    let error = WorkspaceError::GitProbeFailed(GitProbeError::CwdGone(attribution.to_string()));

    let text = error.to_string();

    assert!(
        text.contains("工作目录已不存在：/tmp/removed-worktree"),
        "Display 必须输出 cwd 归因文案，实际：{text}"
    );
    assert!(
        !text.contains("GitUnavailable"),
        "CwdGone 不得回退 Debug 形态，实际：{text}"
    );
}

#[test]
fn workspace_error_display_when_git_probe_unavailable_keeps_legacy_prefix() {
    let error = WorkspaceError::GitProbeFailed(GitProbeError::GitUnavailable);

    assert_eq!(
        error.to_string(),
        "Git 仓库探测失败：GitUnavailable",
        "非 CwdGone 错误必须保持既有前缀文案"
    );
}
