use super::run_post_tool_batch;
use crate::application::activity::ActivityCoordinator;
use async_trait::async_trait;
use hook::{HookDispatchContext, HookInvocation, HookPort};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct RecordingPostBatchHook {
    dispatched_cwds: Mutex<Vec<PathBuf>>,
}

#[async_trait]
impl HookPort for RecordingPostBatchHook {
    async fn dispatch(
        &self,
        _invocation: HookInvocation,
        _cancellation: &dyn hook::CancellationSignal,
    ) -> hook::HookOutcome {
        hook::HookOutcome::proceed()
    }

    async fn dispatch_at(
        &self,
        _invocation: HookInvocation,
        context: HookDispatchContext,
        _cancellation: &dyn hook::CancellationSignal,
    ) -> hook::HookOutcome {
        self.dispatched_cwds
            .lock()
            .unwrap()
            .push(context.cwd().to_path_buf());
        hook::HookOutcome::proceed()
    }
}

#[tokio::test]
async fn post_tool_batch_reads_workspace_root_when_dispatch_begins() {
    let repository = tempfile::tempdir().unwrap();
    let run_git = |args: &[&str], cwd: &std::path::Path| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .unwrap()
            .success()
    };
    assert!(run_git(
        &["init", "--initial-branch=main"],
        repository.path()
    ));
    assert!(run_git(&["config", "user.name", "test"], repository.path()));
    assert!(run_git(
        &["config", "user.email", "test@example.com"],
        repository.path()
    ));
    assert!(run_git(
        &["config", "commit.gpgsign", "false"],
        repository.path()
    ));
    std::fs::write(repository.path().join("README.md"), "init").unwrap();
    assert!(run_git(&["add", "-A"], repository.path()));
    assert!(run_git(&["commit", "-m", "init"], repository.path()));
    let linked_root = repository.path().join("linked");
    assert!(run_git(
        &[
            "worktree",
            "add",
            linked_root.to_str().unwrap(),
            "-b",
            "linked"
        ],
        repository.path()
    ));
    let main_root = repository.path().canonicalize().unwrap();
    let workspace = project::wire_production_workspace(main_root.clone())
        .expect("workspace 初始化成功")
        .into_views();
    workspace
        .control()
        .enter(Some(linked_root), None, None)
        .expect("进入 linked worktree");
    let hook = Arc::new(RecordingPostBatchHook::default());
    let hook_port: Arc<dyn HookPort> = hook.clone();
    workspace
        .control()
        .exit()
        .expect("dispatch 前退出 worktree");

    run_post_tool_batch(
        &hook_port,
        &ActivityCoordinator::production_without_publisher(sdk::RunId::new_v7()),
        &sdk::RunStepId::new_v7(),
        &tokio_util::sync::CancellationToken::new(),
        1,
        1,
        &workspace.read(),
    )
    .await;

    assert_eq!(hook.dispatched_cwds.lock().unwrap().as_slice(), [main_root]);
}
