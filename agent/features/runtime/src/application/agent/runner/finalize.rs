use crate::LOG_TARGET;
use hook::api::HookRunner;
use std::path::Path;
use std::time::Duration;
use tools::{AgentProgressEvent, AgentProgressKind};

/// Agent 循环退出状态
#[derive(Debug, Clone, PartialEq)]
pub enum AgentRunStatus {
    Completed,        // 正常完成
    Cancelled,        // 用户打断
    Failed(String),   // shared loop failure
    TimedOut,         // shared loop timeout
    ApiError(String), // legacy main-loop API error status
}

/// Agent 循环统一结果
#[derive(Debug, Clone)]
pub struct AgentRunOutcome {
    pub status: AgentRunStatus,
    pub turns: usize,
    pub duration: Duration,
    pub role: Option<String>, // 子 agent 有 role，主 loop 为 None
    pub model: String,
}

/// 主 loop 和子 agent 共用的结构化日志摘要
pub fn log_agent_outcome(outcome: &AgentRunOutcome, session_id: &str) {
    log::info!(target: LOG_TARGET,
        "[agent_loop_finished] session={}, status={:?}, turns={}, duration_ms={}, role={}, model={}",
        session_id,
        outcome.status,
        outcome.turns,
        outcome.duration.as_millis(),
        outcome.role.as_deref().unwrap_or("-"),
        outcome.model,
    );
}

/// 子 agent 退出时统一收尾：
///   1. 结构化日志
///   2. SubagentStop hook（含 system_message 转发）
///   3. 不需要恢复 Provider 设置；调用期配置属于不可变 InvocationScope。
#[allow(clippy::too_many_arguments)]
pub(crate) async fn finalize_sub_agent(
    outcome: &AgentRunOutcome,
    hook_runner: &HookRunner,
    session_id: &str,
    prompt: &str,
    system: &str,
    model_spec: Option<&str>,
    output: &str,
    progress_tx: Option<&tokio::sync::mpsc::Sender<AgentProgressEvent>>,
    workspace_root: &Path,
) {
    log_agent_outcome(outcome, session_id);

    let is_error = matches!(
        outcome.status,
        AgentRunStatus::Cancelled
            | AgentRunStatus::Failed(_)
            | AgentRunStatus::TimedOut
            | AgentRunStatus::ApiError(_)
    );
    let hook_results = hook_runner
        .on_subagent_stop(
            prompt,
            system,
            model_spec,
            output,
            outcome.turns,
            is_error,
            workspace_root,
        )
        .await;

    for (_, _, json_output) in &hook_results {
        if let Some(ref output) = json_output {
            if let Some(ref sys_msg) = output.system_message {
                if let Some(tx) = progress_tx {
                    let _ = tx.try_send(AgentProgressEvent {
                        sequence: outcome.turns,
                        kind: AgentProgressKind::Message {
                            text: format!("[hook] {sys_msg}"),
                        },
                    });
                }
            }
        }
    }
}
