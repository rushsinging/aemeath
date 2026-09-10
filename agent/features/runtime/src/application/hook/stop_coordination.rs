//! Stop Hook coordination —— 共享 Loop 触发的 typed decision。
//!
//! Stop Hook outcome 已从 adapter 的专用阻断投影迁入共享 Loop 与 Run 状态机。
//! Coordinator 只消费 HookPort / Hook PL，返回
//! Runtime-owned typed decision；保留 block detail/messages，禁止用 reason 字符串
//! 区分主动 Block 与 ExecutionFailed。Hook 内部三次 retry 仍归 Hook BC。
//!
//! 设计约束：
//! - **typed decision**：`Proceed` | `Block { reason, detail, messages, feedback }`
//!   其中 `reason` 保持 `RuntimeHookReason` variant，`StopHookExecutionFailed`
//!   永远作为 typed variant 保留，绝不转字符串后再识别。
//! - **纯转换**：不解析 stdout / JSON，不维护 Run 状态，不触碰 IO。
//!   Hook BC 已完成所有分类 / JSON 解析；此处仅做类型化搬运。
//! - **feedback materialization**：Main/Sub 都调用本模块的统一异步函数，
//!   使用同一语言、预览截断和长输出落盘规则。

use crate::application::hook::outcome_mapper::{
    map_hook_outcome, RuntimeHookDirective, RuntimeHookReason,
};
use crate::application::loop_engine::LoopEngineError;
use crate::application::run::execution_state::RunExecutionState;
use async_trait::async_trait;
use hook::{
    HookDispatchContext, HookInvocation, HookPoint, HookPort, HookSubscriptionExecutionObserver,
    StopInput,
};
use share::message::{HookNotice, HookNoticeKind, Message};
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Runtime-owned typed stop hook decision.
///
/// 由 `evaluate_stop_hook` 产出，是 Loop 消费 stop hook 结果的唯一入口。
/// Loop 根据此 decision 决定 Proceed（正常完成）或 Block（记录计数、注入 feedback）。
#[derive(Debug, Clone, PartialEq)]
pub enum StopHookDecision {
    /// Stop hook 放行，Run 可以正常完成。
    Proceed,
    /// Stop Hook 执行已被当前 Step 取消，不应向 LLM 注入取消反馈。
    Cancelled,
    /// Stop hook 阻断。携带完整的 typed reason、block detail、feedback 材料。
    /// Block allowance 来自本 Run 冻结的 StopHookPolicy；限额内 continue-with-feedback，首个超限 Block 触发 Run Failed。
    Block(Box<StopHookBlock>),
}

/// Block variant data — boxed to avoid large size difference with `Proceed`.
#[derive(Debug, Clone, PartialEq)]
pub struct StopHookBlock {
    /// 结构化阻断原因（永远 typed，绝不转字符串）。
    pub reason: RuntimeHookReason,
    /// Block detail（触发 Block 的 subscription 与 execution）。
    pub detail: RuntimeHookBlockDetail,
    /// BC 保留的展示消息（按源顺序 1:1 投影，用于 UI 展示）。
    pub messages: Vec<super::outcome_mapper::RuntimeHookDisplayMessage>,
    /// Feedback materialization 所需材料；adapter 在 seam 实现中完成构造。
    pub feedback: HookNoticeMaterial,
}

/// Feedback 材料：由 adapter 层的 `evaluate_stop_hook` 实现消费，
/// 构造通用 `Message::hook_notice` 并注入消息流。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookNoticeMaterial {
    /// LLM 可见的提示文本（含 command/exit_code/reason/stdout/stderr 摘要）。
    pub llm_text: String,
    /// 结构化 Hook notice（TUI 展示与 Session 恢复共用）。
    pub notice: HookNotice,
}

/// Block detail（与 RuntimeHookBlockDetail 语义一致，但作为 StopHookDecision 内嵌类型重新导出）。
pub use crate::application::hook::outcome_mapper::RuntimeHookBlockDetail;

/// Run-scoped dependencies required to execute one Stop Hook.
#[derive(Clone)]
pub struct StopHookExecutionContext {
    hook_port: Arc<dyn HookPort>,
    workspace_read: Arc<dyn project::WorkspaceRead>,
    session_id: String,
    language: String,
    subscription_execution_observer: Option<Arc<dyn HookSubscriptionExecutionObserver>>,
}

impl StopHookExecutionContext {
    pub fn new(
        hook_port: Arc<dyn HookPort>,
        workspace_read: Arc<dyn project::WorkspaceRead>,
        session_id: String,
        language: String,
    ) -> Self {
        Self {
            hook_port,
            workspace_read,
            session_id,
            language,
            subscription_execution_observer: None,
        }
    }

    pub fn with_subscription_execution_observer(
        mut self,
        observer: Arc<dyn HookSubscriptionExecutionObserver>,
    ) -> Self {
        self.subscription_execution_observer = Some(observer);
        self
    }
}

/// Narrow role seam for Stop Hook UI and continuation differences.
#[async_trait]
pub trait StopHookObserver: Send {
    fn stop_hook_execution_context(&self) -> Option<StopHookExecutionContext> {
        None
    }

    fn install_stop_hook_feedback(&mut self, _message: Message) {}

    async fn observe_stop_hook_outcome(
        &mut self,
        _execution: &RunExecutionState,
        _outcome: &StopHookOutcome,
    ) -> Result<(), LoopEngineError> {
        Ok(())
    }
}

pub struct NoopStopHookObserver;

#[async_trait]
impl StopHookObserver for NoopStopHookObserver {}

pub async fn coordinate_stop_hook<O>(
    observer: &mut O,
    execution: &mut RunExecutionState,
    run_steps: usize,
    cancellation: &CancellationToken,
) -> Result<StopHookOutcome, LoopEngineError>
where
    O: StopHookObserver + ?Sized,
{
    let Some(context) = observer.stop_hook_execution_context() else {
        return Ok(StopHookOutcome {
            decision: StopHookDecision::Proceed,
            feedback_message: None,
        });
    };
    let outcome = orchestrate_stop_hook(
        &context.hook_port,
        StopHookContext {
            run_steps,
            workspace_root: context.workspace_read.current_workspace_root(),
            session_id: context.session_id,
            language: context.language,
            subscription_execution_observer: context.subscription_execution_observer,
        },
        cancellation,
    )
    .await;

    if let Some(message) = outcome.feedback_message.clone() {
        observer.install_stop_hook_feedback(message.clone());
        execution.record_step_message(message.clone());
        execution.append_message(message);
    }
    observer
        .observe_stop_hook_outcome(execution, &outcome)
        .await?;
    Ok(outcome)
}

pub struct StopHookContext {
    pub run_steps: usize,
    pub workspace_root: PathBuf,
    pub session_id: String,
    pub language: String,
    pub subscription_execution_observer: Option<Arc<dyn HookSubscriptionExecutionObserver>>,
}

#[derive(Debug, Clone)]
pub struct StopHookOutcome {
    pub decision: StopHookDecision,
    pub feedback_message: Option<Message>,
}

pub(crate) fn hook_point_view(point: HookPoint) -> sdk::HookPointView {
    match point {
        HookPoint::PreToolUse => sdk::HookPointView::PreToolUse,
        HookPoint::UserPromptSubmit => sdk::HookPointView::UserPromptSubmit,
        HookPoint::PreCompact => sdk::HookPointView::PreCompact,
        HookPoint::PermissionRequest => sdk::HookPointView::PermissionRequest,
        HookPoint::Elicitation => sdk::HookPointView::Elicitation,
        HookPoint::UserPromptExpansion => sdk::HookPointView::UserPromptExpansion,
        HookPoint::Stop => sdk::HookPointView::Stop,
        HookPoint::PostToolUse => sdk::HookPointView::PostToolUse,
        HookPoint::PostToolUseFailure => sdk::HookPointView::PostToolUseFailure,
        HookPoint::PostCompact => sdk::HookPointView::PostCompact,
        HookPoint::PostToolBatch => sdk::HookPointView::PostToolBatch,
        HookPoint::ElicitationResult => sdk::HookPointView::ElicitationResult,
        HookPoint::SessionStart => sdk::HookPointView::SessionStart,
        HookPoint::SessionEnd => sdk::HookPointView::SessionEnd,
        HookPoint::SubRunStart => sdk::HookPointView::SubRunStart,
        HookPoint::SubRunStop => sdk::HookPointView::SubRunStop,
        HookPoint::TaskCreated => sdk::HookPointView::TaskCreated,
        HookPoint::TaskCompleted => sdk::HookPointView::TaskCompleted,
        HookPoint::Notification => sdk::HookPointView::Notification,
        HookPoint::InstructionsLoaded => sdk::HookPointView::InstructionsLoaded,
        HookPoint::StopFailure => sdk::HookPointView::StopFailure,
        HookPoint::PermissionDenied => sdk::HookPointView::PermissionDenied,
        HookPoint::ConfigChange => sdk::HookPointView::ConfigChange,
        HookPoint::CwdChanged => sdk::HookPointView::CwdChanged,
        HookPoint::FileChanged => sdk::HookPointView::FileChanged,
        HookPoint::TeammateIdle => sdk::HookPointView::TeammateIdle,
    }
}

pub async fn orchestrate_stop_hook(
    hook_port: &Arc<dyn HookPort>,
    context: StopHookContext,
    cancellation: &CancellationToken,
) -> StopHookOutcome {
    let invocation = HookInvocation::Stop(StopInput {
        run_steps: context.run_steps,
    });
    let mut hook_dispatch_context = HookDispatchContext::new(&context.workspace_root);
    if let Some(observer) = context.subscription_execution_observer {
        hook_dispatch_context =
            hook_dispatch_context.with_subscription_execution_observer(observer);
    }
    let hook_outcome = hook_port
        .dispatch_at(invocation, hook_dispatch_context, cancellation)
        .await;
    if cancellation.is_cancelled()
        || hook_outcome
            .executions
            .iter()
            .any(|execution| matches!(execution.status, hook::HookExecutionStatus::Cancelled))
    {
        return StopHookOutcome {
            decision: StopHookDecision::Cancelled,
            feedback_message: None,
        };
    }
    let dispatch = map_hook_outcome(&hook_outcome);

    let (decision, feedback_message) = match &dispatch.directive {
        RuntimeHookDirective::Block { reason } => {
            let detail = dispatch
                .block_detail
                .clone()
                .expect("Stop hook Block must carry the blocking subscription detail");
            let feedback = materialize_stop_hook_feedback(
                &detail,
                reason,
                &context.session_id,
                &context.language,
            )
            .await;
            let message = Message::hook_notice(
                format!(
                    "<system-reminder>\n{}\n</system-reminder>",
                    feedback.llm_text
                ),
                feedback.notice.clone(),
            );
            (
                StopHookDecision::Block(Box::new(StopHookBlock {
                    reason: reason.clone(),
                    detail,
                    messages: dispatch.messages.clone(),
                    feedback,
                })),
                Some(message),
            )
        }
        _ => (StopHookDecision::Proceed, None),
    };

    StopHookOutcome {
        decision,
        feedback_message,
    }
}

// ─── Feedback materialization helpers ─────────────────────────────

const INLINE_HOOK_OUTPUT_LIMIT: usize = 4_000;
const TUI_STDOUT_PREVIEW_LINES: usize = 3;
const TUI_STDERR_PREVIEW_LINES: usize = 5;

/// Materialize Stop Hook feedback with the same behavior for Main and Sub.
/// Long output is persisted under a per-session temp directory and the model
/// receives the real readable path. When the process layer spilled full output
/// to files (truncation occurred), the spill files are moved into the archive
/// so the persisted copy is the complete hook output, not the 8KB bounded copy.
pub(crate) async fn materialize_stop_hook_feedback(
    detail: &RuntimeHookBlockDetail,
    reason: &RuntimeHookReason,
    session_id: &str,
    language: &str,
) -> HookNoticeMaterial {
    let output_file = archive_hook_output(detail, reason, session_id)
        .await
        .map(|path| path.display().to_string());
    build_stop_hook_feedback(detail, reason, language, output_file)
}

/// Decide what to persist:
/// - spill files present → move them into the archive (full output wins);
/// - no spill but combined bounded output > inline limit → write bounded copy;
/// - otherwise → nothing (inline previews carry the full content).
async fn archive_hook_output(
    detail: &RuntimeHookBlockDetail,
    reason: &RuntimeHookReason,
    session_id: &str,
) -> Option<PathBuf> {
    let has_spill =
        detail.execution.stdout_file.is_some() || detail.execution.stderr_file.is_some();
    let bounded_output = full_hook_output(detail, reason);
    if !has_spill && bounded_output.len() <= INLINE_HOOK_OUTPUT_LIMIT {
        return None;
    }
    let dir = std::env::temp_dir()
        .join("aemeath-hook-results")
        .join(session_id);
    tokio::fs::create_dir_all(&dir).await.ok()?;
    // 文件名带执行序号：同一命令多次 block 各自留档，绝不互相覆盖。
    let stem = sanitized_file_stem(&detail.command);
    let path = dir.join(format!("{stem}-{}.txt", detail.execution_ordinal));
    if has_spill {
        let mut body = String::new();
        body.push_str(&format!(
            "command: {}\nexit_code: {:?}\nreason: {}\n",
            detail.command,
            detail.execution.exit_code,
            format_reason(reason)
        ));
        let stdout_full = match &detail.execution.stdout_file {
            Some(spill) => {
                let text = tokio::fs::read_to_string(spill).await.ok()?;
                let _ = tokio::fs::remove_file(spill).await;
                text
            }
            None => detail.execution.stdout.clone(),
        };
        let stderr_full = match &detail.execution.stderr_file {
            Some(spill) => {
                let text = tokio::fs::read_to_string(spill).await.ok()?;
                let _ = tokio::fs::remove_file(spill).await;
                text
            }
            None => detail.execution.stderr.clone(),
        };
        body.push_str(&format!(
            "\nstdout:\n{stdout_full}\n\nstderr:\n{stderr_full}"
        ));
        tokio::fs::write(&path, body).await.ok()?;
    } else {
        tokio::fs::write(&path, &bounded_output).await.ok()?;
    }
    Some(path)
}

fn build_stop_hook_feedback(
    detail: &RuntimeHookBlockDetail,
    reason: &RuntimeHookReason,
    language: &str,
    output_file: Option<String>,
) -> HookNoticeMaterial {
    let summary = match language {
        "zh" => "Stop hook 阻止了停止。".to_string(),
        _ => "Stop hook prevented stopping.".to_string(),
    };
    let command = detail.command.clone();
    let reason_text = format_reason(reason);
    let (stdout_preview, stdout_truncated) =
        truncate_lines(&detail.execution.stdout, TUI_STDOUT_PREVIEW_LINES);
    let (stderr_preview, stderr_truncated) =
        truncate_lines(&detail.execution.stderr, TUI_STDERR_PREVIEW_LINES);
    let notice = HookNotice {
        point: "Stop".to_string(),
        kind: HookNoticeKind::Blocked,
        summary,
        command,
        exit_code: detail.execution.exit_code,
        reason: reason_text,
        stdout_preview,
        stderr_preview,
        stdout_truncated,
        stderr_truncated,
        output_file,
    };

    let mut llm_text = stop_hook_llm_text_english(&notice);
    if language == "zh" {
        llm_text = llm_text.replace("Stop hook prevented stopping.", "Stop hook 阻止了停止。");
    }
    HookNoticeMaterial { llm_text, notice }
}

fn full_hook_output(detail: &RuntimeHookBlockDetail, reason: &RuntimeHookReason) -> String {
    format!(
        "command: {}\nexit_code: {:?}\nreason: {}\n\nstdout:\n{}\n\nstderr:\n{}",
        detail.command,
        detail.execution.exit_code,
        format_reason(reason),
        detail.execution.stdout,
        detail.execution.stderr,
    )
}

fn sanitized_file_stem(command: &str) -> String {
    let mut stem: String = command
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while stem.contains("--") {
        stem = stem.replace("--", "-");
    }
    let stem = stem.trim_matches('-');
    if stem.is_empty() {
        "hook-output".to_string()
    } else {
        stem.chars().take(80).collect()
    }
}
fn format_reason(reason: &RuntimeHookReason) -> String {
    match reason {
        RuntimeHookReason::ExitCode { code, .. } => format!("exit code {code}"),
        RuntimeHookReason::JsonBlock { reason } => reason.clone(),
        RuntimeHookReason::JsonContinueFalse { stop_reason } => stop_reason
            .clone()
            .unwrap_or_else(|| "hook returned continue:false".to_string()),
        RuntimeHookReason::StopHookExecutionFailed { error }
        | RuntimeHookReason::PolicyBlock { error } => error.clone(),
    }
}

fn stop_hook_llm_text_english(notice: &HookNotice) -> String {
    let mut text = format!(
        "{}\nCommand: {}\nExit code: {}\nReason: {}",
        notice.summary,
        notice.command,
        notice
            .exit_code
            .map_or_else(|| "unknown".to_string(), |code| code.to_string()),
        notice.reason
    );
    if let Some(path) = &notice.output_file {
        text.push_str(&format!(
            "\nFull hook output is saved to {path}; use the Read tool to inspect it."
        ));
    } else {
        if !notice.stderr_preview.trim().is_empty() {
            text.push_str(&format!("\nstderr:\n{}", notice.stderr_preview));
        }
        if !notice.stdout_preview.trim().is_empty() {
            text.push_str(&format!("\nstdout:\n{}", notice.stdout_preview));
        }
    }
    text
}

fn truncate_lines(text: &str, max_lines: usize) -> (String, bool) {
    let lines: Vec<&str> = text.lines().collect();
    let truncated = lines.len() > max_lines;
    (
        lines
            .into_iter()
            .take(max_lines)
            .collect::<Vec<_>>()
            .join("\n"),
        truncated,
    )
}

// ─── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "stop_coordination_tests.rs"]
mod tests;
