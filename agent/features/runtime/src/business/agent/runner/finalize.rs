use hook::api::HookRunner;
use provider::api::LlmClient;
use share::tool::{AgentProgressEvent, AgentProgressKind};
use std::time::Duration;
use crate::LOG_TARGET;

/// Agent 循环退出状态
#[derive(Debug, Clone, PartialEq)]
pub enum AgentRunStatus {
    Completed,        // 正常完成
    Cancelled,        // 用户打断
    TimedOut,         // 子 agent 超时（10 分钟）
    ApiError(String), // LLM API 错误
    MaxTurns,         // 子 agent 达到 max turns
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
///   3. 恢复 client 设置
#[allow(clippy::too_many_arguments)]
pub(crate) async fn finalize_sub_agent(
    outcome: &AgentRunOutcome,
    client: &LlmClient,
    hook_runner: &HookRunner,
    session_id: &str,
    prompt: &str,
    system: &str,
    model_spec: Option<&str>,
    output: &str,
    previous_max_tokens: u32,
    previous_reasoning: bool,
    restore_max_tokens: bool,
    progress_tx: Option<&tokio::sync::mpsc::Sender<AgentProgressEvent>>,
) {
    log_agent_outcome(outcome, session_id);

    let is_error = matches!(
        outcome.status,
        AgentRunStatus::Cancelled | AgentRunStatus::TimedOut | AgentRunStatus::ApiError(_)
    );
    let hook_results = hook_runner
        .on_subagent_stop(prompt, system, model_spec, output, outcome.turns, is_error)
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

    if restore_max_tokens && previous_max_tokens > 0 {
        client.set_max_tokens(previous_max_tokens);
    }
    client.set_reasoning(previous_reasoning);
}
