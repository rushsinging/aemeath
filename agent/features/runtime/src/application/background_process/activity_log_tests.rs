//! format + sink 包装测试（#1962 Agent 等待期过程日志）。

use std::sync::Mutex;
use std::time::SystemTime;

use context::{SessionId, ToolCallIdentityData};
use sdk::{RunId, RunStepId};
use tools::published::agent::{AgentProgressEvent, AgentProgressKind, AgentToolCallProgress};
use tools::published::execution::{ProgressSink, ToolProgressEvent};
use tools::published::sub_run::SubRunTerminalOutcome;

use super::*;
use crate::application::constants::AGENT_ACTIVITY_LOG_LINE_MAX_CHARS;

fn identity(call_suffix: &str) -> ToolCallIdentityData {
    ToolCallIdentityData {
        session_id: SessionId::new("session-1"),
        run_id: RunId::new("run-1"),
        step_id: RunStepId::new("step-1"),
        runtime_call_id: format!("runtime-call-{call_suffix}"),
        provider_call_id: None,
        tool_name: "Agent".to_string(),
        call_index: 0,
        agent: true,
    }
}

fn event(kind: AgentProgressKind) -> AgentProgressEvent {
    AgentProgressEvent {
        source_context: None,
        sequence: 0,
        kind,
    }
}

fn call(id: &str, name: &str) -> AgentToolCallProgress {
    AgentToolCallProgress {
        id: id.to_string(),
        name: name.to_string(),
        input: serde_json::json!({}),
    }
}

/// 独立临时目录 + 空任务日志路径（目录必须先存在）。
fn temp_log_path(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "bgp-activity-{}-{}-{tag}",
        std::process::id(),
        SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("建临时日志目录");
    dir.join("task.log")
}

// ── format ──

#[test]
fn format_started_with_role_and_model() {
    let line = format_agent_activity_line(&AgentProgressKind::Started {
        role: Some("engineer".to_string()),
        model: "claude-test".to_string(),
    })
    .expect("Started 应产出摘要行");
    assert_eq!(line, "started role=engineer model=claude-test");
}

#[test]
fn format_started_without_role() {
    let line = format_agent_activity_line(&AgentProgressKind::Started {
        role: None,
        model: "claude-test".to_string(),
    })
    .expect("Started 无 role 也应产出摘要行");
    assert_eq!(line, "started model=claude-test");
    assert!(!line.contains("role="), "role 缺失时不应出现 role= 占位");
}

#[test]
fn format_tool_calls_joins_names_with_comma() {
    let line = format_agent_activity_line(&AgentProgressKind::ToolCalls {
        calls: vec![call("1", "Bash"), call("2", "Read"), call("3", "Grep")],
    })
    .expect("ToolCalls 应产出摘要行");
    assert_eq!(line, "tool_call: Bash, Read, Grep");
}

#[test]
fn format_tool_result_reports_status_only() {
    let ok = format_agent_activity_line(&AgentProgressKind::ToolResult {
        tool_call_id: "1".to_string(),
        tool_name: "Bash".to_string(),
        output: "lots of stdout text".to_string(),
        content: serde_json::json!({"stdout": "lots of stdout text"}),
        is_error: false,
    })
    .expect("ToolResult 应产出摘要行");
    assert_eq!(ok, "tool_result: Bash ok");

    let err = format_agent_activity_line(&AgentProgressKind::ToolResult {
        tool_call_id: "2".to_string(),
        tool_name: "Read".to_string(),
        output: "permission denied".to_string(),
        content: serde_json::json!(null),
        is_error: true,
    })
    .expect("ToolResult(err) 应产出摘要行");
    assert_eq!(err, "tool_result: Read err");
    assert!(
        !err.contains("permission denied"),
        "不得把整段 output 塞进摘要行"
    );
}

#[test]
fn format_terminal_reports_outcome_kind_only() {
    let cases = [
        (SubRunTerminalOutcome::Completed, "terminal: completed"),
        (
            SubRunTerminalOutcome::Failed {
                error: "boom long summary text".to_string(),
            },
            "terminal: failed",
        ),
        (SubRunTerminalOutcome::Cancelled, "terminal: cancelled"),
    ];
    for (outcome, expected) in cases {
        let line = format_agent_activity_line(&AgentProgressKind::Terminal { outcome })
            .expect("Terminal 应产出摘要行");
        assert_eq!(line, expected);
        assert!(
            !line.contains("boom long summary text"),
            "Terminal 只写 outcome 种类，NEVER 写终态总结正文"
        );
    }
}

#[test]
fn format_skips_flood_kinds() {
    let flood_kinds = [
        AgentProgressKind::Thinking {
            text: "thinking out loud".to_string(),
        },
        AgentProgressKind::Message {
            text: "a long assistant message".to_string(),
        },
        AgentProgressKind::ToolOutput {
            tool_name: "Bash".to_string(),
            text: "streamed chunk".to_string(),
        },
    ];
    for kind in flood_kinds {
        assert!(
            format_agent_activity_line(&kind).is_none(),
            "洪水类 kind 应跳过：{kind:?}"
        );
    }
}

#[test]
fn format_truncates_line_to_char_budget() {
    let long_name = "A".repeat(AGENT_ACTIVITY_LOG_LINE_MAX_CHARS * 2);
    let line = format_agent_activity_line(&AgentProgressKind::ToolCalls {
        calls: vec![call("1", &long_name)],
    })
    .expect("超长 ToolCalls 也应产出摘要行");
    assert!(
        line.chars().count() <= AGENT_ACTIVITY_LOG_LINE_MAX_CHARS,
        "行必须按字符截断到预算内，实际 {}",
        line.chars().count()
    );
    assert!(line.ends_with('…'), "截断应补省略号");
}

// ── sink 包装 ──

#[derive(Default)]
struct RecordingSink {
    agent_events: Mutex<Vec<AgentProgressEvent>>,
    tool_events: Mutex<Vec<ToolProgressEvent>>,
}

impl ProgressSink for RecordingSink {
    fn emit(&self, event: AgentProgressEvent) {
        self.agent_events.lock().unwrap().push(event);
    }
    fn emit_tool_stream(&self, event: ToolProgressEvent) {
        self.tool_events.lock().unwrap().push(event);
    }
}

#[test]
fn emit_appends_activity_line_to_indirect_task_log() {
    let log_path = temp_log_path("emit");
    let supervisor = Arc::new(BackgroundProcessSupervisor::new());
    supervisor.register_indirect_log(
        share::ids::BackgroundProcessId::new_v7(),
        log_path.clone(),
        identity("1"),
        "Agent run",
        tokio_util::sync::CancellationToken::new(),
        SystemTime::now(),
    );

    let recording = Arc::new(RecordingSink::default());
    let sink = wrap_with_activity_log(recording.clone(), Some(supervisor), "runtime-call-1");
    sink.emit(event(AgentProgressKind::Started {
        role: Some("engineer".to_string()),
        model: "claude-test".to_string(),
    }));

    let text = std::fs::read_to_string(&log_path).expect("任务日志可读");
    assert_eq!(text, "started role=engineer model=claude-test\n");
    assert_eq!(
        recording.agent_events.lock().unwrap().len(),
        1,
        "inner sink 仍应收到原事件"
    );

    let _ = std::fs::remove_file(&log_path);
}

#[test]
fn emit_without_supervisor_only_forwards() {
    let recording = Arc::new(RecordingSink::default());
    let sink = wrap_with_activity_log(recording.clone(), None, "runtime-call-none");
    sink.emit(event(AgentProgressKind::Terminal {
        outcome: SubRunTerminalOutcome::Completed,
    }));

    let events = recording.agent_events.lock().unwrap();
    assert_eq!(events.len(), 1, "无 supervisor 时仅转发");
    assert_eq!(
        events[0].kind,
        AgentProgressKind::Terminal {
            outcome: SubRunTerminalOutcome::Completed
        }
    );
}

#[test]
fn emit_tool_stream_never_writes_task_log() {
    let log_path = temp_log_path("stream");
    let supervisor = Arc::new(BackgroundProcessSupervisor::new());
    supervisor.register_indirect_log(
        share::ids::BackgroundProcessId::new_v7(),
        log_path.clone(),
        identity("2"),
        "Agent run",
        tokio_util::sync::CancellationToken::new(),
        SystemTime::now(),
    );

    let recording = Arc::new(RecordingSink::default());
    let sink = wrap_with_activity_log(recording.clone(), Some(supervisor), "runtime-call-2");
    sink.emit_tool_stream(ToolProgressEvent {
        text: "chunk\n".to_string(),
    });

    assert_eq!(recording.tool_events.lock().unwrap().len(), 1);
    let text = std::fs::read_to_string(&log_path).unwrap_or_default();
    assert!(text.is_empty(), "emit_tool_stream 只转 inner，不写任务日志");
}
