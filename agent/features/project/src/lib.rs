pub(crate) const LOG_TARGET: &str = "aemeath:agent:project";
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
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    /// 每测试独立的 RAII 临时目录，在 Drop 时自动清理。
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(prefix: &str) -> Self {
            let base = std::env::temp_dir();
            for _ in 0..100 {
                let nonce = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("system clock must be after the Unix epoch")
                    .as_nanos();
                let sequence = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
                let path = base.join(format!(
                    "aemeath-pj-{prefix}-{}-{nonce}-{sequence}",
                    std::process::id()
                ));
                match std::fs::create_dir(&path) {
                    Ok(()) => {
                        return Self {
                            path: path.canonicalize().unwrap(),
                        };
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("failed to create {}: {error}", path.display()),
                }
            }
            panic!("failed to allocate a unique temporary directory");
        }

        fn path(&self) -> &Path {
            &self.path
        }

        /// 在 temp 目录内初始化隔离于用户配置的最小 git repo。
        fn init_git(&self) {
            let hooks = self.path.join("empty-hooks");
            std::fs::create_dir(&hooks).unwrap();
            let status = std::process::Command::new("git")
                .args(["init", "--initial-branch=main"])
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env(
                    "GIT_CONFIG_GLOBAL",
                    self.path.join("unavailable-global-config"),
                )
                .env("GIT_CONFIG_COUNT", "3")
                .env("GIT_CONFIG_KEY_0", "core.hooksPath")
                .env("GIT_CONFIG_VALUE_0", &hooks)
                .env("GIT_CONFIG_KEY_1", "commit.gpgsign")
                .env("GIT_CONFIG_VALUE_1", "false")
                .env("GIT_CONFIG_KEY_2", "tag.gpgsign")
                .env("GIT_CONFIG_VALUE_2", "false")
                .current_dir(&self.path)
                .status()
                .expect("git init 失败（git 是否已安装？）");
            assert!(status.success(), "git init 退出码非 0");
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    // ---- 三视图 / 派生隔离 ----

    #[test]
    fn production_wiring_exposes_three_views_from_one_backing() {
        let tmp = TempDir::new("views");
        let wiring = wire_production_workspace(tmp.path().to_path_buf()).unwrap();

        let read = wiring.read();
        let control = wiring.control();
        let persist = wiring.persist();

        assert_eq!(read.current_path_base(), tmp.path());
        control.change_directory(tmp.path().to_path_buf()).unwrap();
        assert_eq!(read.current_path_base(), tmp.path());
        assert_eq!(
            persist.snapshot().path_base,
            tmp.path().display().to_string()
        );
    }

    #[test]
    fn derived_wiring_has_isolated_state() {
        let tmp = TempDir::new("derived");
        let parent = wire_production_workspace(tmp.path().to_path_buf()).unwrap();
        let child = parent.derive_isolated();
        let child_path = tmp.path().join("child-only");

        std::fs::create_dir_all(&child_path).unwrap();
        child
            .control()
            .change_directory(child_path.clone())
            .unwrap();

        assert_eq!(child.read().current_path_base(), child_path);
        assert_eq!(parent.read().current_path_base(), tmp.path());
    }

    // ---- #894: production wiring 对 Git / NonGit 初始化并返回 Result / 结构化错误 ----

    /// #894: production wiring 必须返回 `Result`；成功路径经 `WorkspaceRead`
    /// 暴露完整 `project_identity` 与稳定 `workspace_id`。在临时 git repo 中验证。
    #[test]
    fn production_wiring_returns_result_and_exposes_identity() {
        let tmp = TempDir::new("identity");
        tmp.init_git();
        let wiring: WorkspaceWiring =
            wire_production_workspace(tmp.path().to_path_buf()).expect("git repo 应初始化成功");
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
        let missing = PathBuf::from("/definitely/not/here/aemeath-894-xyz");
        let result = wire_production_workspace(missing);
        assert!(
            matches!(result, Err(WorkspaceInitError::PathNotFound { .. })),
            "缺失路径应返回结构化 PathNotFound 错误"
        );
    }

    /// 提供文件路径（非目录）必须返回 WorkspaceInitError::NotDirectory。
    #[test]
    fn production_wiring_rejects_file_path_as_not_directory() {
        let tmp = TempDir::new("nondir");
        let file_path = tmp.path().join("a_file.txt");
        std::fs::write(&file_path, "content").unwrap();
        let result = wire_production_workspace(file_path);
        assert!(
            matches!(result, Err(WorkspaceInitError::NotDirectory { .. })),
            "文件路径应返回结构化 NotDirectory 错误"
        );
    }

    /// #894: 合法非 git 目录必须以 NonGit identity 初始化——
    /// `git_common_dir` 为 `None` 且 `in_worktree()` 恒为 false。
    #[test]
    fn production_wiring_initializes_non_git_directory() {
        let tmp = TempDir::new("nongit");
        let wiring: WorkspaceWiring =
            wire_production_workspace(tmp.path().to_path_buf()).expect("普通目录应初始化成功");
        let read = wiring.read();
        assert!(
            read.project_identity().git_common_dir.is_none(),
            "NonGit 目录不应记录 git common dir"
        );
        assert!(!read.in_worktree(), "NonGit 目录 in_worktree 恒为 false");
    }

    // ---- #941: wire_production_workspace 日志契约 ----
    // 真实入口必须在进入时记录 enter、在成功/失败退出时记录 exit；
    // 日志只携带安全的结果类别 / worktree kind，绝不包含完整路径。

    thread_local! {
        static CAPTURED_PROJECT_LOGS: std::cell::RefCell<Vec<(log::Level, String)>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    struct ProjectCapturingLogger;

    impl log::Log for ProjectCapturingLogger {
        fn enabled(&self, _metadata: &log::Metadata) -> bool {
            true
        }

        fn log(&self, record: &log::Record) {
            if record.target() == LOG_TARGET {
                let level = record.level();
                let payload = format!("{}", record.args());
                CAPTURED_PROJECT_LOGS.with(|cell| cell.borrow_mut().push((level, payload)));
            }
        }

        fn flush(&self) {}
    }

    /// 进程内仅安装一次捕获 logger（`log::set_boxed_logger` 只能成功一次）。
    /// 捕获存储为 thread-local，因此并行测试互不可见。
    fn install_project_capturing_logger() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            log::set_boxed_logger(Box::new(ProjectCapturingLogger))
                .expect("capturing logger must install exactly once per process");
            log::set_max_level(log::LevelFilter::Trace);
        });
    }

    /// 取走并清空本线程已捕获的日志，避免 OS 线程复用带来的残留。
    fn drain_project_logs() -> Vec<(log::Level, String)> {
        CAPTURED_PROJECT_LOGS.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
    }

    /// #941: 成功路径必须记录 enter 与 success exit；success 日志携带安全的
    /// worktree kind（NonGit/Primary/Linked），且完整路径绝不出现。
    #[test]
    fn wire_production_workspace_logs_enter_and_success_without_paths() {
        install_project_capturing_logger();
        let _ = drain_project_logs();
        let tmp = TempDir::new("log-success");
        let wiring = wire_production_workspace(tmp.path().to_path_buf());
        assert!(wiring.is_ok(), "普通目录应初始化成功");
        drop(wiring);

        let logs = drain_project_logs();
        let joined = logs
            .iter()
            .map(|(_, m)| m.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            logs.iter().any(|(_, m)| m.contains("enter")),
            "应记录 enter：{joined}"
        );
        assert!(
            logs.iter().any(|(_, m)| m.contains("success")),
            "应记录 success exit：{joined}"
        );
        assert!(
            logs.iter().any(|(_, m)| {
                m.contains("NonGit") || m.contains("Primary") || m.contains("Linked")
            }),
            "success 日志应包含安全的 worktree kind：{joined}"
        );

        let path_str = tmp.path().display().to_string();
        assert!(
            !joined.contains(&path_str),
            "日志不得包含完整路径 {path_str}：{joined}"
        );
    }

    /// #941: 失败路径必须记录 enter 与 failure exit；failure 日志携带安全的
    /// 错误类别（如 PathNotFound），且完整路径绝不出现。
    #[test]
    fn wire_production_workspace_logs_enter_and_failure_without_paths() {
        install_project_capturing_logger();
        let _ = drain_project_logs();
        let missing = PathBuf::from("/definitely/not/here/aemeath-941-log-fail");
        let result = wire_production_workspace(missing.clone());
        assert!(result.is_err());

        let logs = drain_project_logs();
        let joined = logs
            .iter()
            .map(|(_, m)| m.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            logs.iter().any(|(_, m)| m.contains("enter")),
            "应记录 enter：{joined}"
        );
        assert!(
            logs.iter().any(|(_, m)| m.contains("failure")),
            "应记录 failure exit：{joined}"
        );
        assert!(
            logs.iter().any(|(_, m)| m.contains("PathNotFound")),
            "failure 日志应包含安全的错误类别：{joined}"
        );

        let path_str = missing.display().to_string();
        assert!(
            !joined.contains(&path_str),
            "日志不得包含完整路径 {path_str}：{joined}"
        );
    }
}
