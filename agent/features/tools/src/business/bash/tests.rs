use super::bash_result::{exit_status_description, preview, signal_name, PREVIEW_MAX};
use super::*;
use crate::api::ToolResources;
use serde_json::json;
use share::tool::{AgentProgressEvent, AgentProgressKind};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn test_bash_persists_cd_for_subsequent_write_path_base() {
    let workspace = tempdir().unwrap();
    let worktree = workspace.path().join(".worktrees/bug35");
    std::fs::create_dir_all(&worktree).unwrap();
    let ws = project::api::WorkspaceService::new(workspace.path().to_path_buf());
    let ctx = ToolExecutionContext {
        workspace: ws.clone(),
        run_id: "test-run".to_string(),
        cancel: CancellationToken::new(),
        read_files: Arc::new(Mutex::new(HashSet::new())),
        resources: ToolResources {
            agent_runner: None,
            registry: None,
            memory_config: share::config::MemoryConfig::default(),
            lang: "en".to_string(),
            allow_all: true,
        },
        session_reminders: None,
        plan_mode: None,
        max_tool_concurrency: 4,
        max_agent_concurrency: 4,
        agent_semaphore: Arc::new(Semaphore::new(4)),
        progress_tx: None,
        parent_session_id: None,
    };

    let result = BashTool
        .call(
            json!({ "command": format!("cd {}", worktree.display()) }),
            &ctx,
        )
        .await;

    assert!(!result.is_error);
    use project::api::WorkspaceRead;
    assert_eq!(ws.current_path_base(), worktree);
    // workspace_root 应该保持为原来的 git 仓库根目录，不会因为 cd 到非 git 目录而改变
    assert_eq!(ws.current_workspace_root(), workspace.path());
}

#[tokio::test]
async fn test_bash_display_field_contains_stdout_not_message() {
    // 回归：Bash result 的 output 应包含 stdout（通过 display 字段），
    // 而非 "Command executed successfully" 元信息。
    let workspace = tempdir().unwrap();
    let ws = project::api::WorkspaceService::new(workspace.path().to_path_buf());
    let ctx = ToolExecutionContext {
        workspace: ws.clone(),
        run_id: "test-run".to_string(),
        cancel: CancellationToken::new(),
        read_files: Arc::new(Mutex::new(HashSet::new())),
        resources: ToolResources {
            agent_runner: None,
            registry: None,
            memory_config: share::config::MemoryConfig::default(),
            lang: "en".to_string(),
            allow_all: true,
        },
        session_reminders: None,
        plan_mode: None,
        max_tool_concurrency: 4,
        max_agent_concurrency: 4,
        agent_semaphore: Arc::new(Semaphore::new(4)),
        progress_tx: None,
        parent_session_id: None,
    };

    let result = BashTool
        .call(json!({ "command": "echo hello_world_12345" }), &ctx)
        .await;

    assert!(!result.is_error);
    // output 应包含 stdout 内容，而非 "Command executed successfully"
    assert!(
        result.text.contains("hello_world_12345"),
        "output 应包含 stdout，实际: {}",
        result.text
    );
    assert!(
        !result.text.contains("Command executed successfully"),
        "output 不应是元信息 'Command executed successfully'，实际: {}",
        result.text
    );
    // data 中应有 stdout 字段
    let data = result.data.expect("应有 data");
    assert_eq!(data.stdout, "hello_world_12345", "data.stdout 应为命令输出");
    // #500: result text 末尾应附带 [cwd: {path}]
    assert!(
        result.text.contains("[cwd: "),
        "output 应包含 [cwd: ...]，实际: {}",
        result.text
    );
}

#[tokio::test]
async fn test_bash_result_cwd_reflects_cd() {
    // AC2: cd 后 result 中的 cwd 应反映新目录
    let workspace = tempdir().unwrap();
    let subdir = workspace.path().join("subdir");
    std::fs::create_dir_all(&subdir).unwrap();
    let ws = project::api::WorkspaceService::new(workspace.path().to_path_buf());
    let ctx = ToolExecutionContext {
        workspace: ws.clone(),
        run_id: "test-run".to_string(),
        cancel: CancellationToken::new(),
        read_files: Arc::new(Mutex::new(HashSet::new())),
        resources: ToolResources {
            agent_runner: None,
            registry: None,
            memory_config: share::config::MemoryConfig::default(),
            lang: "en".to_string(),
            allow_all: true,
        },
        session_reminders: None,
        plan_mode: None,
        max_tool_concurrency: 4,
        max_agent_concurrency: 4,
        agent_semaphore: Arc::new(Semaphore::new(4)),
        progress_tx: None,
        parent_session_id: None,
    };

    let result = BashTool
        .call(
            json!({ "command": format!("cd {}", subdir.display()) }),
            &ctx,
        )
        .await;

    assert!(!result.is_error);
    assert!(
        result.text.contains(&subdir.display().to_string()),
        "cd 后 result 应包含新目录路径，实际: {}",
        result.text
    );
}

#[tokio::test]
async fn test_bash_result_cwd_on_failed_command() {
    // AC6: 命令失败（非零 exit code）时 result 也带 cwd
    let workspace = tempdir().unwrap();
    let ws = project::api::WorkspaceService::new(workspace.path().to_path_buf());
    let ctx = ToolExecutionContext {
        workspace: ws.clone(),
        run_id: "test-run".to_string(),
        cancel: CancellationToken::new(),
        read_files: Arc::new(Mutex::new(HashSet::new())),
        resources: ToolResources {
            agent_runner: None,
            registry: None,
            memory_config: share::config::MemoryConfig::default(),
            lang: "en".to_string(),
            allow_all: true,
        },
        session_reminders: None,
        plan_mode: None,
        max_tool_concurrency: 4,
        max_agent_concurrency: 4,
        agent_semaphore: Arc::new(Semaphore::new(4)),
        progress_tx: None,
        parent_session_id: None,
    };

    let result = BashTool.call(json!({ "command": "false" }), &ctx).await;

    assert!(result.is_error);
    assert!(
        result.text.contains("[cwd: "),
        "失败命令 result 也应包含 [cwd: ...]，实际: {}",
        result.text
    );
}

#[tokio::test]
async fn test_bash_result_cwd_on_empty_output() {
    // AC5: stdout + stderr 均为空时的 "Command executed successfully" 也带 cwd
    let workspace = tempdir().unwrap();
    let ws = project::api::WorkspaceService::new(workspace.path().to_path_buf());
    let ctx = ToolExecutionContext {
        workspace: ws.clone(),
        run_id: "test-run".to_string(),
        cancel: CancellationToken::new(),
        read_files: Arc::new(Mutex::new(HashSet::new())),
        resources: ToolResources {
            agent_runner: None,
            registry: None,
            memory_config: share::config::MemoryConfig::default(),
            lang: "en".to_string(),
            allow_all: true,
        },
        session_reminders: None,
        plan_mode: None,
        max_tool_concurrency: 4,
        max_agent_concurrency: 4,
        agent_semaphore: Arc::new(Semaphore::new(4)),
        progress_tx: None,
        parent_session_id: None,
    };

    let result = BashTool.call(json!({ "command": "true" }), &ctx).await;

    assert!(!result.is_error);
    assert!(
        result.text.contains("Command executed successfully"),
        "空输出应有 success 消息，实际: {}",
        result.text
    );
    assert!(
        result.text.contains("[cwd: "),
        "空输出 result 也应包含 [cwd: ...]，实际: {}",
        result.text
    );
}

#[tokio::test]
async fn test_bash_streams_stdout_via_progress_tx() {
    use tokio::sync::mpsc;

    let workspace = tempdir().unwrap();
    let ws = project::api::WorkspaceService::new(workspace.path().to_path_buf());
    let (tx, mut rx) = mpsc::channel::<AgentProgressEvent>(256);
    let ctx = ToolExecutionContext {
        workspace: ws.clone(),
        run_id: "test-run".to_string(),
        cancel: CancellationToken::new(),
        read_files: Arc::new(Mutex::new(HashSet::new())),
        resources: ToolResources {
            agent_runner: None,
            registry: None,
            memory_config: share::config::MemoryConfig::default(),
            lang: "en".to_string(),
            allow_all: true,
        },
        session_reminders: None,
        plan_mode: None,
        max_tool_concurrency: 4,
        max_agent_concurrency: 4,
        agent_semaphore: Arc::new(Semaphore::new(4)),
        progress_tx: Some(tx),
        parent_session_id: None,
    };

    let result = BashTool
        .call(
            json!({ "command": "echo progress_stream_test_marker" }),
            &ctx,
        )
        .await;

    assert!(!result.is_error);

    // Drop ctx (which owns the original Sender) so that once the spawned
    // stdout reader finishes and drops its clone, the channel is fully closed
    // and rx.recv() will return None.
    drop(ctx);

    // Collect all progress events
    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }

    assert!(
        !events.is_empty(),
        "progress_tx should have received at least one event"
    );

    // All collected text fragments concatenated should contain the echoed marker
    let all_text: String = events
        .iter()
        .filter_map(|ev| match &ev.kind {
            AgentProgressKind::ToolOutput { tool_name, text } if tool_name == "Bash" => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect();

    assert!(
        all_text.contains("progress_stream_test_marker"),
        "progress events should contain echoed output, got: {:?}",
        events
    );

    // No event should contain the internal CWD marker
    for ev in &events {
        if let AgentProgressKind::ToolOutput { tool_name, text } = &ev.kind {
            assert_eq!(tool_name, "Bash");
            assert!(
                !text.contains("__AEMEATH_CWD__"),
                "progress event must not contain __AEMEATH_CWD__ marker: {}",
                text
            );
        }
    }

    // Sequence must be monotonically increasing and > 0
    for ev in &events {
        assert!(
            ev.sequence > 0,
            "progress event sequence must be > 0, got {}",
            ev.sequence
        );
    }
}

#[tokio::test]
async fn test_bash_no_progress_tx_still_works() {
    let workspace = tempdir().unwrap();
    let ws = project::api::WorkspaceService::new(workspace.path().to_path_buf());
    let ctx = ToolExecutionContext {
        workspace: ws.clone(),
        run_id: "test-run".to_string(),
        cancel: CancellationToken::new(),
        read_files: Arc::new(Mutex::new(HashSet::new())),
        resources: ToolResources {
            agent_runner: None,
            registry: None,
            memory_config: share::config::MemoryConfig::default(),
            lang: "en".to_string(),
            allow_all: true,
        },
        session_reminders: None,
        plan_mode: None,
        max_tool_concurrency: 4,
        max_agent_concurrency: 4,
        agent_semaphore: Arc::new(Semaphore::new(4)),
        progress_tx: None,
        parent_session_id: None,
    };

    let result = BashTool
        .call(json!({ "command": "echo no_channel_test_98765" }), &ctx)
        .await;

    assert!(!result.is_error);
    assert!(
        result.text.contains("no_channel_test_98765"),
        "output should contain echoed text even without progress_tx, got: {}",
        result.text
    );
}

// ---- Issue #286: exit code 映射 + 信号终止诊断 ----

#[test]
fn test_exit_status_description_normal_exit() {
    // 正常退出码 → 返回实际码 + "exit code N"
    let status = std::process::Command::new("bash")
        .arg("-c")
        .arg("exit 42")
        .status()
        .unwrap();
    let (code, detail) = exit_status_description(&status);
    assert_eq!(code, 42);
    assert_eq!(detail, "exit code 42");
}

#[test]
fn test_exit_status_description_success() {
    let status = std::process::Command::new("bash")
        .arg("-c")
        .arg("true")
        .status()
        .unwrap();
    let (code, detail) = exit_status_description(&status);
    assert_eq!(code, 0);
    assert_eq!(detail, "exit code 0");
}

#[cfg(unix)]
#[test]
fn test_exit_status_description_signal_termination() {
    // 信号终止 → exit_code=-1, detail 包含 signal 信息
    let status = std::process::Command::new("bash")
        .arg("-c")
        .arg("kill -9 $$")
        .status()
        .unwrap();
    assert!(
        status.code().is_none(),
        "signal termination should have no exit code"
    );
    let (code, detail) = exit_status_description(&status);
    assert_eq!(code, -1);
    assert!(
        detail.starts_with("signal 9"),
        "detail should indicate SIGKILL, got: {detail}"
    );
    assert!(
        detail.contains("SIGKILL"),
        "detail should include signal name, got: {detail}"
    );
}

#[test]
fn test_signal_name_known_signals() {
    assert_eq!(signal_name(9), "SIGKILL");
    assert_eq!(signal_name(15), "SIGTERM");
    assert_eq!(signal_name(2), "SIGINT");
}

#[test]
fn test_signal_name_unknown_signal() {
    assert_eq!(signal_name(255), "UNKNOWN");
    assert_eq!(signal_name(0), "UNKNOWN");
}

#[test]
fn test_preview_no_truncation() {
    // 短字符串（< PREVIEW_MAX）原样返回
    let s = "short stdout";
    assert_eq!(preview(s), "short stdout");
}

#[test]
fn test_preview_truncation_with_marker() {
    // 长字符串（>= PREVIEW_MAX）按 char boundary 截断，附加截断标记
    let s: String = "a".repeat(PREVIEW_MAX + 100);
    let result = preview(&s);
    assert!(
        result.starts_with(&"a".repeat(PREVIEW_MAX)),
        "should keep first PREVIEW_MAX bytes"
    );
    assert!(
        result.contains("...[truncated"),
        "should include truncation marker"
    );
    assert!(
        result.contains("100 bytes"),
        "should report truncated byte count, got: {result}"
    );
}

#[test]
fn test_preview_respects_utf8_char_boundary() {
    // UTF-8 多字节字符：PREVIEW_MAX 落在多字节字符中间时，必须按 char boundary 截断
    // 汉字 "中" 占 3 字节；构造一个 PREVIEW_MAX = 512 全部由汉字组成的字符串
    let s: String = "中".repeat(PREVIEW_MAX);
    // 中占 3 字节，总长 PREVIEW_MAX * 3 > PREVIEW_MAX，必然触发截断
    // 但 PREVIEW_MAX = 512 不是 3 的倍数，可能在某个字符中间；切到最近的 char boundary
    let result = preview(&s);
    // 不应该 panic
    assert!(
        result.contains("...[truncated"),
        "should include truncation marker"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_bash_command_killed_by_signal_reports_signal_in_message() {
    // 回归 #286：被信号杀死的命令不应只报 "exit code -1"，
    // 而应包含 signal 信息。
    let workspace = tempdir().unwrap();
    let ws = project::api::WorkspaceService::new(workspace.path().to_path_buf());
    let ctx = ToolExecutionContext {
        workspace: ws.clone(),
        run_id: "test-run".to_string(),
        cancel: CancellationToken::new(),
        read_files: Arc::new(Mutex::new(HashSet::new())),
        resources: ToolResources {
            agent_runner: None,
            registry: None,
            memory_config: share::config::MemoryConfig::default(),
            lang: "en".to_string(),
            allow_all: true,
        },
        session_reminders: None,
        plan_mode: None,
        max_tool_concurrency: 4,
        max_agent_concurrency: 4,
        agent_semaphore: Arc::new(Semaphore::new(4)),
        progress_tx: None,
        parent_session_id: None,
    };

    let result = BashTool
        .call(json!({ "command": "kill -9 $$" }), &ctx)
        .await;

    assert!(result.is_error);
    // 消息应包含 "signal" 而非无信息的 "exit code -1"
    assert!(
        result.text.contains("signal"),
        "error message should contain signal info, got: {}",
        result.text
    );
    assert!(
        result.text.contains("SIGKILL"),
        "error message should contain signal name, got: {}",
        result.text
    );
}
