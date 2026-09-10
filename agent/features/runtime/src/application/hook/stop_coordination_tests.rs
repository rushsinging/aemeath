//! Tests for stop hook coordination.
//!
//! #1248 Task 6: Verify typed decision variants, Proceed/Block/ExecutionFailed,
//! and that string-based variant discrimination is impossible.
//!
//! Uses real hook dispatchers (via `hook::build_dispatcher`) to avoid
//! cross-feature direct construction of internal hook types.

use super::*;
use crate::application::hook::outcome_mapper::{RuntimeHookExecutionStatus, RuntimeHookReason};
use share::config::hooks::{HookEntry, HookEvent, HooksConfig};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Default)]
struct RecordingWorkspaceHook {
    dispatched_cwds: Mutex<Vec<PathBuf>>,
}

#[async_trait]
impl HookPort for RecordingWorkspaceHook {
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

struct StopContextObserver {
    context: StopHookExecutionContext,
}

#[async_trait]
impl StopHookObserver for StopContextObserver {
    fn stop_hook_execution_context(&self) -> Option<StopHookExecutionContext> {
        Some(self.context.clone())
    }
}

/// Helper: build a dispatcher that always returns Continue.
fn continue_hook_port() -> Arc<dyn HookPort> {
    let mut events = std::collections::HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: "true".to_string(),
            timeout: 5,
        }],
    );
    Arc::new(
        hook::build_dispatcher(&share::config::domain::snapshot::ConfigSnapshot::new(
            share::config::Config {
                hooks: HooksConfig {
                    events,
                    ..HooksConfig::default()
                },
                ..share::config::Config::default()
            },
        ))
        .unwrap(),
    )
}

/// Helper: build a dispatcher that always blocks (exit code 2).
fn always_blocking_hook_port() -> Arc<dyn HookPort> {
    let mut events = std::collections::HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: "echo blocked; exit 2".to_string(),
            timeout: 5,
        }],
    );
    Arc::new(
        hook::build_dispatcher(&share::config::domain::snapshot::ConfigSnapshot::new(
            share::config::Config {
                hooks: HooksConfig {
                    events,
                    ..HooksConfig::default()
                },
                ..share::config::Config::default()
            },
        ))
        .unwrap(),
    )
}

#[tokio::test]
async fn stop_hook_reads_workspace_root_when_dispatch_begins() {
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
    let hook = Arc::new(RecordingWorkspaceHook::default());
    let hook_port: Arc<dyn HookPort> = hook.clone();
    let mut observer = StopContextObserver {
        context: StopHookExecutionContext::new(
            hook_port,
            workspace.read(),
            "test-session".to_string(),
            "en".to_string(),
        ),
    };
    workspace
        .control()
        .exit()
        .expect("dispatch 前退出 worktree");
    let mut execution = RunExecutionState::new();

    let outcome = coordinate_stop_hook(
        &mut observer,
        &mut execution,
        1,
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .expect("Stop Hook 协调成功");

    assert!(matches!(outcome.decision, StopHookDecision::Proceed));
    assert_eq!(hook.dispatched_cwds.lock().unwrap().as_slice(), [main_root]);
}

#[tokio::test]
async fn continue_decision_preserves_typed_dispatch_for_mapping() {
    let port = continue_hook_port();
    let outcome = orchestrate_stop_hook(
        &port,
        StopHookContext {
            run_steps: 3,
            workspace_root: std::path::PathBuf::from("/tmp"),
            session_id: "test-session".to_string(),
            language: "en".to_string(),
            subscription_execution_observer: None,
        },
        &tokio_util::sync::CancellationToken::new(),
    )
    .await;

    assert!(matches!(outcome.decision, StopHookDecision::Proceed));
    assert!(outcome.feedback_message.is_none());
}

#[tokio::test]
async fn cancelled_stop_hook_does_not_materialize_llm_feedback() {
    let port = always_blocking_hook_port();
    let cancellation = tokio_util::sync::CancellationToken::new();
    cancellation.cancel();

    let outcome = orchestrate_stop_hook(
        &port,
        StopHookContext {
            run_steps: 3,
            workspace_root: std::path::PathBuf::from("/tmp"),
            session_id: "test-session".to_string(),
            language: "en".to_string(),
            subscription_execution_observer: None,
        },
        &cancellation,
    )
    .await;

    assert!(matches!(outcome.decision, StopHookDecision::Cancelled));
    assert!(outcome.feedback_message.is_none());
}

#[tokio::test]
async fn block_outcome_materializes_feedback_message_once() {
    let port = always_blocking_hook_port();
    let outcome = orchestrate_stop_hook(
        &port,
        StopHookContext {
            run_steps: 3,
            workspace_root: std::path::PathBuf::from("/tmp"),
            session_id: "test-session".to_string(),
            language: "zh".to_string(),
            subscription_execution_observer: None,
        },
        &tokio_util::sync::CancellationToken::new(),
    )
    .await;

    assert!(matches!(outcome.decision, StopHookDecision::Block(_)));
    let message = outcome
        .feedback_message
        .expect("blocked outcome must carry one feedback message");
    assert_eq!(message.source(), share::message::MessageSource::Hook);
    assert!(message.text_content().contains("<system-reminder>"));
}

#[tokio::test]
async fn block_returns_typed_reason_not_string() {
    let port = always_blocking_hook_port();
    let outcome = orchestrate_stop_hook(
        &port,
        StopHookContext {
            run_steps: 3,
            workspace_root: std::path::PathBuf::from("/tmp"),
            session_id: "test-session".to_string(),
            language: "en".to_string(),
            subscription_execution_observer: None,
        },
        &tokio_util::sync::CancellationToken::new(),
    )
    .await;

    match outcome.decision {
        StopHookDecision::Block(block) => {
            // reason MUST be typed, not a flattened string
            assert!(
                matches!(block.reason, RuntimeHookReason::ExitCode { .. }),
                "reason must be typed ExitCode, got {:?}",
                block.reason
            );
            // detail must carry a command
            assert!(!block.detail.command.is_empty());
            // feedback material must contain llm_text and payload
            assert!(!block.feedback.llm_text.is_empty());
        }
        other => panic!("expected Block, got {:?}", other),
    }
}

#[test]
fn block_decision_cannot_be_confused_with_execution_failed_by_string() {
    // Verify that two Block decisions with different variant types but
    // similar text content remain distinguishable at compile time via
    // pattern matching.
    let json_block_reason = RuntimeHookReason::JsonBlock {
        reason: "spawn failed".to_string(),
    };
    let exec_failed_reason = RuntimeHookReason::StopHookExecutionFailed {
        error: "spawn failed".to_string(),
    };

    // Same error text — but different variants. Type system enforces
    // that any consumer must match explicitly.
    assert_ne!(
        std::mem::discriminant(&json_block_reason),
        std::mem::discriminant(&exec_failed_reason),
        "JsonBlock and StopHookExecutionFailed must be distinct variants"
    );
}

#[test]
fn feedback_material_truncates_long_output() {
    use crate::application::hook::outcome_mapper::{RuntimeHookBlockDetail, RuntimeHookExecution};

    let detail = RuntimeHookBlockDetail {
        command: "test".to_string(),
        execution_ordinal: 1,
        execution: RuntimeHookExecution {
            status: RuntimeHookExecutionStatus::Blocked,
            attempts: 1,
            exit_code: Some(1),
            stdout: "line1\nline2\nline3\nline4\nline5\nline6\n".to_string(),
            stderr: "err1\nerr2\n".to_string(),
            stdout_file: None,
            stderr_file: None,
            duration: std::time::Duration::from_secs(1),
        },
    };
    let reason = RuntimeHookReason::ExitCode {
        code: 1,
        stderr: String::new(),
    };
    let feedback = super::build_stop_hook_feedback(&detail, &reason, "en", None);
    let payload = feedback.notice;

    // stdout preview should be truncated to TUI_STDOUT_PREVIEW_LINES (3)
    let stdout_lines: Vec<&str> = payload.stdout_preview.lines().collect();
    assert_eq!(
        stdout_lines.len(),
        3,
        "stdout should be truncated to 3 lines"
    );
    assert!(payload.stdout_truncated, "stdout_truncated should be true");

    // stderr should not be truncated (2 lines < 5)
    assert!(!payload.stderr_truncated, "stderr should not be truncated");
}

#[tokio::test]
async fn long_feedback_materializes_real_readable_file_for_sub_and_main() {
    use crate::application::hook::outcome_mapper::{RuntimeHookBlockDetail, RuntimeHookExecution};

    let session_id = format!("stop-hook-test-{}", uuid::Uuid::now_v7());
    let stdout = "x".repeat(INLINE_HOOK_OUTPUT_LIMIT + 1);
    let detail = RuntimeHookBlockDetail {
        command: "check-agent-stop.sh".to_string(),
        execution_ordinal: 1,
        execution: RuntimeHookExecution {
            status: RuntimeHookExecutionStatus::Blocked,
            attempts: 1,
            exit_code: Some(1),
            stdout: stdout.clone(),
            stderr: String::new(),
            stdout_file: None,
            stderr_file: None,
            duration: std::time::Duration::from_secs(1),
        },
    };
    let reason = RuntimeHookReason::ExitCode {
        code: 1,
        stderr: String::new(),
    };

    let feedback = super::materialize_stop_hook_feedback(&detail, &reason, &session_id, "zh").await;
    let path = feedback
        .notice
        .output_file
        .as_deref()
        .expect("long output must be persisted");

    assert_ne!(path, "[pending — adapter will resolve]");
    assert!(std::path::Path::new(path).is_file());
    assert!(feedback.llm_text.contains(path));
    assert!(tokio::fs::read_to_string(path)
        .await
        .unwrap()
        .contains(&stdout));

    let _ = tokio::fs::remove_file(path).await;
    let _ = tokio::fs::remove_dir_all(
        std::env::temp_dir()
            .join("aemeath-hook-results")
            .join(session_id),
    )
    .await;
}

#[tokio::test]
async fn spill_files_become_full_output_archive_instead_of_truncated_copy() {
    use crate::application::hook::outcome_mapper::{RuntimeHookBlockDetail, RuntimeHookExecution};

    // 模拟进程层 spill：stderr 全量 16KB 在临时文件，内存缓冲只有 8KB 残本。
    let session_id = format!("stop-hook-spill-{}", uuid::Uuid::now_v7());
    let full_stderr = "F".repeat(16 * 1024);
    let spill_path = std::env::temp_dir().join(format!("{}-stderr.txt", session_id));
    std::fs::write(&spill_path, &full_stderr).expect("plant spill file");
    let detail = RuntimeHookBlockDetail {
        command: "big-gate.sh".to_string(),
        execution_ordinal: 1,
        execution: RuntimeHookExecution {
            status: RuntimeHookExecutionStatus::Blocked,
            attempts: 1,
            exit_code: Some(2),
            stdout: String::new(),
            stderr: "F".repeat(8 * 1024),
            stdout_file: None,
            stderr_file: Some(spill_path.clone()),
            duration: std::time::Duration::from_secs(1),
        },
    };
    let reason = RuntimeHookReason::ExitCode {
        code: 2,
        stderr: String::new(),
    };

    let feedback = super::materialize_stop_hook_feedback(&detail, &reason, &session_id, "zh").await;
    let path = feedback
        .notice
        .output_file
        .as_deref()
        .expect("spill 输出必须落盘");
    let archived = tokio::fs::read_to_string(path).await.unwrap();
    assert!(
        archived.contains(&full_stderr),
        "落盘正文必须包含全量 stderr（而非 8KB 截断残本）"
    );
    assert!(!spill_path.exists(), "spill 临时文件被 move 后不得残留");
    let _ = tokio::fs::remove_file(path).await;
    let _ = tokio::fs::remove_dir_all(
        std::env::temp_dir()
            .join("aemeath-hook-results")
            .join(session_id),
    )
    .await;
}

#[tokio::test]
async fn repeated_blocks_of_same_command_never_overwrite_each_other() {
    use crate::application::hook::outcome_mapper::{RuntimeHookBlockDetail, RuntimeHookExecution};

    let session_id = format!("stop-hook-seq-{}", uuid::Uuid::now_v7());
    let make_detail = |ordinal: u32, marker: &str| RuntimeHookBlockDetail {
        command: "same-gate.sh".to_string(),
        execution_ordinal: ordinal,
        execution: RuntimeHookExecution {
            status: RuntimeHookExecutionStatus::Blocked,
            attempts: 1,
            exit_code: Some(2),
            stdout: format!(
                "{marker}\n{}",
                "x".repeat(super::INLINE_HOOK_OUTPUT_LIMIT + 1)
            ),
            stderr: String::new(),
            stdout_file: None,
            stderr_file: None,
            duration: std::time::Duration::from_secs(1),
        },
    };
    let reason = RuntimeHookReason::ExitCode {
        code: 2,
        stderr: String::new(),
    };

    let first = super::materialize_stop_hook_feedback(
        &make_detail(1, "FIRST-BLOCK"),
        &reason,
        &session_id,
        "zh",
    )
    .await;
    let second = super::materialize_stop_hook_feedback(
        &make_detail(2, "SECOND-BLOCK"),
        &reason,
        &session_id,
        "zh",
    )
    .await;

    let first_path = first
        .notice
        .output_file
        .as_deref()
        .expect("first block file");
    let second_path = second
        .notice
        .output_file
        .as_deref()
        .expect("second block file");
    assert_ne!(first_path, second_path, "两次 block 必须落两个不同文件");
    let first_text = tokio::fs::read_to_string(first_path).await.unwrap();
    let second_text = tokio::fs::read_to_string(second_path).await.unwrap();
    assert!(first_text.contains("FIRST-BLOCK"), "第一次内容不得被覆盖");
    assert!(second_text.contains("SECOND-BLOCK"));
    let _ = tokio::fs::remove_file(first_path).await;
    let _ = tokio::fs::remove_file(second_path).await;
    let _ = tokio::fs::remove_dir_all(
        std::env::temp_dir()
            .join("aemeath-hook-results")
            .join(session_id),
    )
    .await;
}
