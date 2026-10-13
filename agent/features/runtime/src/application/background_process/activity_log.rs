//! Agent 过程日志（#1962）：后台任务等待期的过程摘要行。
//!
//! 写入点在 [`ProgressSink::emit`] 包装内——execute 返回占位后 progress
//! forward 循环 500ms 即超时被杀，转后台后 TUI SubRunActivity 也断链，
//! 因此只有随 tool context 留在后台 join_handle 的 sink 才能持续落盘。

use std::sync::Arc;

use tools::published::agent::{AgentProgressEvent, AgentProgressKind};
use tools::published::execution::ProgressSink;
use tools::published::sub_run::SubRunTerminalOutcome;

use crate::application::background_process::supervisor::BackgroundProcessSupervisor;
use crate::application::constants::AGENT_ACTIVITY_LOG_LINE_MAX_CHARS;

/// 把关键 Activity 归约为任务日志单行摘要；洪水类 kind 返回 None 跳过。
///
/// 只写"发生了什么"，NEVER 写终态总结正文或整段工具输出（硬预算防刷屏）。
pub(crate) fn format_agent_activity_line(kind: &AgentProgressKind) -> Option<String> {
    let line = match kind {
        AgentProgressKind::Started { role, model } => match role {
            Some(role) => format!("started role={role} model={model}"),
            None => format!("started model={model}"),
        },
        AgentProgressKind::ToolCalls { calls } => {
            let names: Vec<&str> = calls.iter().map(|call| call.name.as_str()).collect();
            format!("tool_call: {}", names.join(", "))
        }
        AgentProgressKind::ToolResult {
            tool_name,
            is_error,
            ..
        } => {
            let status = if *is_error { "err" } else { "ok" };
            format!("tool_result: {tool_name} {status}")
        }
        AgentProgressKind::Terminal { outcome } => format!(
            "terminal: {}",
            match outcome {
                SubRunTerminalOutcome::Completed => "completed",
                SubRunTerminalOutcome::Failed { .. } => "failed",
                SubRunTerminalOutcome::Cancelled => "cancelled",
            }
        ),
        // Thinking / Message / ToolOutput 内容长且洪水，跳过（防刷屏）。
        AgentProgressKind::Thinking { .. }
        | AgentProgressKind::Message { .. }
        | AgentProgressKind::ToolOutput { .. } => return None,
    };
    Some(truncate_to_budget(line))
}

/// 按字符截断到硬预算；截断时补省略号（不拆 UTF-8 字符）。
fn truncate_to_budget(line: String) -> String {
    if line.chars().count() <= AGENT_ACTIVITY_LOG_LINE_MAX_CHARS {
        return line;
    }
    let mut truncated: String = line
        .chars()
        .take(AGENT_ACTIVITY_LOG_LINE_MAX_CHARS - 1)
        .collect();
    truncated.push('…');
    truncated
}

/// 包装 inner sink：emit 时先写 Activity 摘要行进任务日志，再转发 inner。
struct AgentActivityLogSink {
    inner: Arc<dyn ProgressSink>,
    supervisor: Option<Arc<BackgroundProcessSupervisor>>,
    runtime_call_id: String,
}

impl ProgressSink for AgentActivityLogSink {
    fn emit(&self, event: AgentProgressEvent) {
        if let (Some(supervisor), Some(line)) = (
            self.supervisor.as_ref(),
            format_agent_activity_line(&event.kind),
        ) {
            supervisor.append_activity_line_for_runtime_call(&self.runtime_call_id, &line);
        }
        self.inner.emit(event);
    }

    fn emit_tool_stream(&self, event: tools::published::execution::ToolProgressEvent) {
        self.inner.emit_tool_stream(event);
    }
}

/// 有 supervisor 才包装活动日志；无后台运行时直接返回原 sink。
pub(crate) fn wrap_with_activity_log(
    inner: Arc<dyn ProgressSink>,
    supervisor: Option<Arc<BackgroundProcessSupervisor>>,
    runtime_call_id: impl Into<String>,
) -> Arc<dyn ProgressSink> {
    let Some(supervisor) = supervisor else {
        return inner;
    };
    Arc::new(AgentActivityLogSink {
        inner,
        supervisor: Some(supervisor),
        runtime_call_id: runtime_call_id.into(),
    })
}

#[cfg(test)]
#[path = "activity_log_tests.rs"]
mod tests;
