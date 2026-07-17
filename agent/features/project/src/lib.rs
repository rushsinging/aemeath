/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:agent:project";

mod adapters;
mod domain;

pub use adapters::wiring::{wire_production_workspace, WorkspaceViews, WorkspaceWiring};
pub use domain::state::PreparedWorkspaceRestore;
pub use domain::types::{
    GitOperationError, GitProbeError, WorkspaceControl, WorkspaceError, WorkspaceFrame,
    WorkspaceInitError, WorkspacePersist, WorkspaceRead, WorkspaceRestoreError,
};
pub use share::session_types::{ProjectIdentity, WorkspaceId, WorktreeKind};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_wiring_exposes_three_views_from_one_backing() {
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        let wiring = wire_production_workspace(cwd.clone()).unwrap();

        let read = wiring.read();
        let control = wiring.control();
        let persist = wiring.persist();

        assert_eq!(read.current_path_base(), cwd);
        control.change_directory(cwd.clone()).unwrap();
        assert_eq!(read.current_path_base(), cwd);
        assert_eq!(persist.snapshot().path_base, cwd.display().to_string());
    }

    #[test]
    fn derived_wiring_has_isolated_state() {
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        let parent = wire_production_workspace(cwd.clone()).unwrap();
        let child = parent.derive_isolated();
        let child_path = cwd.join("child-only");

        std::fs::create_dir_all(&child_path).unwrap();
        child
            .control()
            .change_directory(child_path.clone())
            .unwrap();

        assert_eq!(child.read().current_path_base(), child_path);
        assert_eq!(parent.read().current_path_base(), cwd);
    }

    // ---- #894: production wiring 对 Git / NonGit 初始化并返回 Result / 结构化错误 ----

    /// #894: production wiring 必须返回 `Result`；成功路径经 `WorkspaceRead`
    /// 暴露完整 `project_identity` 与稳定 `workspace_id`。当前 worktree 位于 git
    /// repo，identity 必含 canonical git common dir。
    #[test]
    fn production_wiring_returns_result_and_exposes_identity() {
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        let wiring: WorkspaceWiring =
            wire_production_workspace(cwd.clone()).expect("git repo cwd 应初始化成功");
        let read = wiring.read();
        assert!(
            read.project_identity().git_common_dir.is_some(),
            "git 目录应记录 canonical common dir"
        );
        assert!(
            !read.workspace_id().as_str().is_empty(),
            "workspace_id 应为非空 opaque 标识"
        );
    }

    /// #894: 不存在的路径必须返回结构化 `WorkspaceInitError`，
    /// NEVER 以未校验路径建立 wiring。
    #[test]
    fn production_wiring_rejects_missing_path_with_structured_error() {
        let missing = std::path::PathBuf::from("/definitely/not/here/aemeath-894-xyz");
        let result = wire_production_workspace(missing);
        assert!(
            matches!(result, Err(WorkspaceInitError::PathNotFound { .. })),
            "缺失路径应返回结构化 PathNotFound 错误"
        );
    }

    /// #894: 合法非 git 目录必须以 NonGit identity 初始化——
    /// `git_common_dir` 为 `None` 且 `in_worktree()` 恒为 false。
    #[test]
    fn production_wiring_initializes_non_git_directory() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let tmp = std::env::temp_dir().join(format!("aemeath_894_nongit_{}", nanos));
        std::fs::create_dir_all(&tmp).unwrap();
        let canonical = tmp.canonicalize().unwrap();

        let wiring: WorkspaceWiring =
            wire_production_workspace(canonical.clone()).expect("普通目录应初始化成功");
        let read = wiring.read();
        assert!(
            read.project_identity().git_common_dir.is_none(),
            "NonGit 目录不应记录 git common dir"
        );
        assert!(!read.in_worktree(), "NonGit 目录 in_worktree 恒为 false");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
