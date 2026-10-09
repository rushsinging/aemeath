use std::sync::Arc;
use std::time::{Duration, SystemTime};

use context::{
    CleanupConfirmation as ReceiptCleanupConfirmation, ToolCallIdentityData,
    ToolReceiptMutationData, ToolTerminalReceiptData,
};
use share::ids::BackgroundProcessId;
use tools::published::execution::ToolExecutionOutcome as PublishedToolOutcome;
use tools::published::execution::{
    CancellationDeclaration, CancellationSignal, CleanupConfirmation, ToolExecutionContext,
    ToolExecutionPort, ToolInvocation,
};
use tools::ToolCatalogSnapshot;

use crate::application::constants::DEFAULT_GRACE;
use crate::application::context::coordination::ContextCoordinator;

#[derive(Clone)]
pub(crate) struct ToolExecutionSupervisor {
    execution: Arc<dyn ToolExecutionPort>,
    catalog: ToolCatalogSnapshot,
    context: ContextCoordinator,
    grace: Duration,
    /// #252 PR2 通知链路装配：session 级后台进程运行时（Main Run 注入；
    /// Sub Run / 测试为 None，spawn body 只推进 receipt）。
    background: Option<
        Arc<crate::application::background_process::session_runtime::BackgroundProcessRuntime>,
    >,
}

pub(crate) struct SupervisedToolCall {
    pub identity: ToolCallIdentityData,
    pub invocation: ToolInvocation,
    pub context: ToolExecutionContext,
    pub input_preview: String,
    pub run_deadline: Option<SystemTime>,
    pub cancellation: Arc<dyn CancellationSignal>,
    /// per-call child cancellation：deadline 到期或用户取消时由 supervisor
    /// 触发，经 `context.cancellation()` 传播给 Cooperative 工具。
    pub child_cancellation: tokio_util::sync::CancellationToken,
    /// 前台等待阈值：超过即自动转后台（占位结果 + 异步回注）。
    /// `None` 或 `Some(0)` 表示禁用后台化（纯快路径）。随 RunConfigSnapshot 冻结。
    pub background_threshold: Option<Duration>,
}

impl ToolExecutionSupervisor {
    pub(crate) fn new(
        execution: Arc<dyn ToolExecutionPort>,
        catalog: ToolCatalogSnapshot,
        context: ContextCoordinator,
    ) -> Self {
        Self {
            execution,
            catalog,
            context,
            grace: DEFAULT_GRACE,
            background: None,
        }
    }

    /// 注入 session 级后台进程运行时（Main Run 传 Some，#252 PR2；
    /// Sub Run / 测试传 None——spawn body 只推进 receipt）。
    pub(crate) fn with_background_runtime(
        mut self,
        runtime: Option<
            Arc<crate::application::background_process::session_runtime::BackgroundProcessRuntime>,
        >,
    ) -> Self {
        self.background = runtime;
        self
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn with_grace(mut self, grace: Duration) -> Self {
        self.grace = grace;
        self
    }

    /// 超阈值转后台：receipt 推进 Backgrounded，spawn detached 驱动在
    /// 后台等待真实终态（唯一取消源 = deadline 快照），前台立即返回占位结果。
    async fn move_to_background(
        &self,
        mut call: SupervisedToolCall,
        mut join_handle: tokio::task::JoinHandle<PublishedToolOutcome>,
        deadline_snapshot: Option<SystemTime>,
        cancellation_declaration: CancellationDeclaration,
        started: std::time::Instant,
        direct: Option<DirectDispatch>,
    ) -> Result<(PublishedToolOutcome, Duration), ToolExecutionSupervisorError> {
        // #252 PR2：有 session 级账本时以账本登记的 task id 为唯一身份
        // （占位、receipt、通知同源）；无装配（Sub Run / 测试）则临时生成。
        // #1890 直绑派发：id 前移到派发时生成（与任务日志文件名同源），
        // 路径随记录入账。
        // #1890：转后台一律带任务日志文件——直绑派发用前移 id 与已建文件；
        // 非直绑（非流式 / Agent）此刻建文件，终态由 notify_terminal 兜底
        // append（文件成为全部后台任务 logs 查询的统一真相源）。
        let task_id = match (self.background.as_ref(), direct) {
            (Some(runtime), Some(dd)) => runtime.supervisor().register_direct(
                dd.process_id,
                dd.path,
                call.identity.clone(),
                invocation_summary_text(&call),
                call.child_cancellation.clone(),
                dispatch_started_time(started),
            ),
            (Some(runtime), None) => {
                let process_id = BackgroundProcessId::new_v7();
                match runtime.open_direct_log(process_id.clone()) {
                    Some(log) => runtime.supervisor().register_direct(
                        process_id,
                        log.path().to_path_buf(),
                        call.identity.clone(),
                        invocation_summary_text(&call),
                        call.child_cancellation.clone(),
                        dispatch_started_time(started),
                    ),
                    None => runtime.supervisor().register_with_cancellation(
                        call.identity.clone(),
                        invocation_summary_text(&call),
                        call.child_cancellation.clone(),
                        dispatch_started_time(started),
                    ),
                }
            }
            (None, _) => BackgroundProcessId::new_v7(),
        };
        let task_log_path = self
            .background
            .as_ref()
            .and_then(|runtime| runtime.supervisor().log_file_of(&task_id));
        log::info!(
            target: crate::LOG_TARGET,
            "tool moved to background: run_id={} step_id={} call_id={} tool={} task_id={} elapsed_ms={} deadline_snapshot={:?}",
            call.identity.run_id,
            call.identity.step_id,
            call.identity.runtime_call_id,
            call.identity.tool_name,
            task_id.as_str(),
            started.elapsed().as_millis(),
            deadline_snapshot,
        );
        self.context
            .advance_tool_receipt(ToolReceiptMutationData::backgrounded(call.identity.clone()))
            .await?;

        let driver_context = self.context.clone();
        let grace = self.grace;
        let identity = call.identity.clone();
        let child_cancellation = call.child_cancellation.clone();
        let task_id_for_logs = task_id.clone();
        // PR2 通知链路：转后台即标记 Backgrounded（带 deadline 快照）。
        let background_runtime = self.background.clone();
        if let Some(runtime) = background_runtime.as_ref() {
            if let Err(error) = runtime
                .supervisor()
                .mark_backgrounded(&task_id, deadline_snapshot)
            {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "background process mark_backgrounded failed: task_id={} error={error:?}",
                    task_id.as_str(),
                );
            }
        }
        tokio::spawn(async move {
            let outcome = match deadline_snapshot {
                Some(deadline) => {
                    let wait = deadline
                        .duration_since(SystemTime::now())
                        .unwrap_or_default();
                    tokio::select! {
                        result = &mut join_handle => join_result_to_outcome(result),
                        _ = tokio::time::sleep(wait) => {
                            log::warn!(
                                target: crate::LOG_TARGET,
                                "background process reached deadline snapshot: run_id={} step_id={} call_id={} tool={} task_id={}",
                                identity.run_id,
                                identity.step_id,
                                identity.runtime_call_id,
                                identity.tool_name,
                                task_id_for_logs.as_str(),
                            );
                            child_cancellation.cancel();
                            cancellation_outcome(cancellation_declaration, join_handle, grace, true).await
                        }
                    }
                }
                None => join_result_to_outcome(join_handle.await),
            };
            let terminal = terminal_receipt(&outcome);
            log::info!(
                target: crate::LOG_TARGET,
                "background process terminal: run_id={} step_id={} call_id={} tool={} task_id={} outcome={:?}",
                identity.run_id,
                identity.step_id,
                identity.runtime_call_id,
                identity.tool_name,
                task_id_for_logs.as_str(),
                terminal.outcome,
            );
            // #252 PR2：终态推进监督器账本并路由通知（无装配时只推进 receipt）。
            if let Some(runtime) = background_runtime.as_ref() {
                runtime
                    .notify_terminal(
                        &driver_context,
                        &task_id_for_logs,
                        terminal_kind_from_outcome(&terminal),
                        terminal_output_text(&outcome),
                    )
                    .await;
            }
            if let Err(error) = driver_context
                .advance_tool_receipt(ToolReceiptMutationData::terminal(identity, terminal))
                .await
            {
                log::error!(
                    target: crate::LOG_TARGET,
                    "background process terminal receipt failed: task_id={} error={error:?}",
                    task_id_for_logs.as_str(),
                );
            }
        });

        let placeholder = placeholder_tool_result(&task_id, task_log_path.as_deref());
        call.background_threshold = None; // 已转后台，防止重复判定
        Ok((placeholder, started.elapsed()))
    }

    pub(crate) async fn execute(
        &self,
        call: SupervisedToolCall,
    ) -> Result<(PublishedToolOutcome, Duration), ToolExecutionSupervisorError> {
        let descriptor = self
            .catalog
            .find(&call.invocation.tool_name)
            .ok_or_else(|| {
                ToolExecutionSupervisorError::ToolUnavailable(call.invocation.tool_name.to_string())
            })?;
        self.context
            .advance_tool_receipt(ToolReceiptMutationData::pending(
                call.identity.clone(),
                call.input_preview.clone(),
            ))
            .await?;
        log::debug!(
            target: crate::LOG_TARGET,
            "tool dispatch accepted: run_id={} step_id={} call_id={} tool={} index={}",
            call.identity.run_id,
            call.identity.step_id,
            call.identity.runtime_call_id,
            call.identity.tool_name,
            call.identity.call_index,
        );
        self.context
            .advance_tool_receipt(ToolReceiptMutationData::running(call.identity.clone()))
            .await?;
        log::debug!(
            target: crate::LOG_TARGET,
            "tool execution started: run_id={} step_id={} call_id={} tool={} timeout_secs={} cancellation={:?}",
            call.identity.run_id,
            call.identity.step_id,
            call.identity.runtime_call_id,
            call.identity.tool_name,
            descriptor.timeout_secs,
            descriptor.cancellation,
        );
        let started = std::time::Instant::now();

        let effective_deadline = earliest_deadline(
            call.invocation.execution_scope.deadline(),
            call.run_deadline,
            Some(SystemTime::now() + Duration::from_secs(descriptor.timeout_secs)),
        );
        log::debug!(
            target: crate::LOG_TARGET,
            "tool execution cancellation identity: run_id={} step_id={} call_id={} tool={} caller_cancelled={} context_cancelled={} declared={:?}",
            call.identity.run_id,
            call.identity.step_id,
            call.identity.runtime_call_id,
            call.identity.tool_name,
            call.cancellation.is_cancelled(),
            call.context.cancellation().is_cancelled(),
            descriptor.cancellation,
        );
        log::debug!(
            target: crate::LOG_TARGET,
            "tool execution awaiting terminal: run_id={} step_id={} call_id={} tool={} caller_cancelled={} effective_deadline={:?}",            call.identity.run_id,
            call.identity.step_id,
            call.identity.runtime_call_id,
            call.identity.tool_name,
            call.cancellation.is_cancelled(),
            effective_deadline
        );

        let backgrounding = call
            .background_threshold
            .filter(|threshold| threshold > &Duration::ZERO);
        // #1890 输出直绑装配：工具声明直绑 + 会话启用后台化 + 监督器/
        // session 就绪时，派发即前移生成进程 id 并创建任务日志文件
        //（文件名与账本 id 同源）；快路径完成删文件，转后台同 id 入账。
        let mut direct_dispatch = backgrounding
            .filter(|_| descriptor.is_background_log_direct())
            .and_then(|_| {
                let runtime = self.background.as_ref()?;
                let process_id = BackgroundProcessId::new_v7();
                let log = runtime.open_direct_log(process_id.clone())?;
                Some(DirectDispatch {
                    process_id,
                    path: log.path().to_path_buf(),
                    log,
                })
            });
        let exec_context = match direct_dispatch.as_ref() {
            Some(dd) => call
                .context
                .clone()
                .with_background_log_path(dd.path.clone()),
            None => call.context.clone(),
        };
        // spawn-first：执行体独立于前台等待，转后台时不 abort 继续运行。
        let mut join_handle = spawn_tool_execution(
            Arc::clone(&self.execution),
            call.invocation.clone(),
            exec_context,
        );

        let outcome = match effective_deadline {
            Some(deadline) => {
                let wait = deadline
                    .duration_since(SystemTime::now())
                    .unwrap_or_default();
                tokio::select! {
                    result = &mut join_handle => join_result_to_outcome(result),
                    _ = call.cancellation.cancelled() => {
                        log::debug!(
                            target: crate::LOG_TARGET,
                            "tool supervisor observed caller cancellation: run_id={} step_id={} call_id={} tool={} elapsed_ms={}",
                            call.identity.run_id,
                            call.identity.step_id,
                            call.identity.runtime_call_id,
                            call.identity.tool_name,
                            started.elapsed().as_millis()
                        );
                        call.child_cancellation.cancel();
                        cancellation_outcome(descriptor.cancellation, join_handle, self.grace, false).await
                    }
                    _ = tokio::time::sleep(wait) => {
                        log::debug!(
                            target: crate::LOG_TARGET,
                            "tool supervisor observed deadline: run_id={} step_id={} call_id={} tool={} elapsed_ms={}",
                            call.identity.run_id,
                            call.identity.step_id,
                            call.identity.runtime_call_id,
                            call.identity.tool_name,
                            started.elapsed().as_millis()
                        );
                        call.child_cancellation.cancel();
                        cancellation_outcome(descriptor.cancellation, join_handle, self.grace, true).await
                    }
                    _ = backgrounding_sleep(backgrounding) => {
                        return self
                            .move_to_background(
                                call,
                                join_handle,
                                effective_deadline,
                                descriptor.cancellation,
                                started,
                                direct_dispatch,
                            )
                            .await;
                    }
                }
            }
            None => tokio::select! {
                result = &mut join_handle => join_result_to_outcome(result),
                _ = call.cancellation.cancelled() => {
                    log::debug!(
                        target: crate::LOG_TARGET,
                        "tool supervisor observed caller cancellation: run_id={} step_id={} call_id={} tool={} elapsed_ms={}",
                        call.identity.run_id,
                        call.identity.step_id,
                        call.identity.runtime_call_id,
                        call.identity.tool_name,
                        started.elapsed().as_millis()
                    );
                    call.child_cancellation.cancel();
                    cancellation_outcome(descriptor.cancellation, join_handle, self.grace, false).await
                }
                _ = backgrounding_sleep(backgrounding) => {
                    return self
                        .move_to_background(
                            call,
                            join_handle,
                            effective_deadline,
                            descriptor.cancellation,
                            started,
                            direct_dispatch,
                        )
                        .await;
                }
            },
        };

        let terminal = terminal_receipt(&outcome);
        let unconfirmed = matches!(outcome, PublishedToolOutcome::CancellationUnconfirmed(_));
        if unconfirmed {
            log::warn!(
                target: crate::LOG_TARGET,
                "tool cleanup unconfirmed: run_id={} step_id={} call_id={} tool={} elapsed_ms={} outcome={:?}",
                call.identity.run_id,
                call.identity.step_id,
                call.identity.runtime_call_id,
                call.identity.tool_name,
                started.elapsed().as_millis(),
                terminal.outcome,
            );
        } else {
            log::debug!(
                target: crate::LOG_TARGET,
                "tool execution terminal: run_id={} step_id={} call_id={} tool={} elapsed_ms={} outcome={:?}",
                call.identity.run_id,
                call.identity.step_id,
                call.identity.runtime_call_id,
                call.identity.tool_name,
                started.elapsed().as_millis(),
                terminal.outcome,
            );
        }
        self.context
            .advance_tool_receipt(ToolReceiptMutationData::terminal(call.identity, terminal))
            .await?;
        // #1890 快路径清理：前台完成的直绑任务输出已直接进 tool_result，
        // 任务日志文件删除不留垃圾（转后台路径已在 move_to_background
        // 消费 direct_dispatch）。
        if let Some(dd) = direct_dispatch.take() {
            if let Err(error) = dd.log.remove() {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "fast-path direct log remove failed: path={:?} error={error:?}",
                    dd.path
                );
            }
        }
        // #1666：与 [tool execution terminal] 日志同源的执行耗时，随 outcome
        // 返回给调用方进入事件流（ChatEvent::ToolResult.duration_ms）。
        Ok((outcome, started.elapsed()))
    }
}

/// #1890 直绑派发件：前移生成的进程 id + 任务日志文件。
///
/// 快路径（前台完成）→ 删除文件；转后台 → `move_to_background` 消费
/// （id 与路径入账，文件成为 logs 查询真相源）。
struct DirectDispatch {
    process_id: BackgroundProcessId,
    path: std::path::PathBuf,
    log: crate::application::background_process::log_file::TaskLogFile,
}

/// spawn 执行体：owned context move 进独立 task，执行体生命周期独立于前台等待。
fn spawn_tool_execution(
    execution: Arc<dyn ToolExecutionPort>,
    invocation: ToolInvocation,
    context: ToolExecutionContext,
) -> tokio::task::JoinHandle<PublishedToolOutcome> {
    tokio::spawn(async move { execution.execute(invocation, &context).await })
}

/// 阈值等待分支：`None` / 零值时永不触发（前台不转后台）。
async fn backgrounding_sleep(threshold: Option<Duration>) {
    match threshold {
        Some(duration) if duration > Duration::ZERO => tokio::time::sleep(duration).await,
        _ => std::future::pending::<()>().await,
    }
}

/// JoinHandle 结果归一：task panic 投影为 Internal failure。
fn join_result_to_outcome(
    result: Result<PublishedToolOutcome, tokio::task::JoinError>,
) -> PublishedToolOutcome {
    match result {
        Ok(outcome) => outcome,
        Err(join_error) => PublishedToolOutcome::failure(
            tools::ToolErrorKind::Internal,
            format!("tool execution task failed: {join_error}"),
        ),
    }
}

/// 占位 tool result：转后台后立即发布给 LLM 的合法成功结果。
/// 工具派发时刻（`Instant`）换算为墙钟 `SystemTime`：进程开始时刻 =
/// 当前墙钟 - 已流逝时长。任务记录的自派发时刻起算的时长依赖该值。
fn dispatch_started_time(started: std::time::Instant) -> std::time::SystemTime {
    std::time::SystemTime::now()
        .checked_sub(started.elapsed())
        .unwrap_or_else(std::time::SystemTime::now)
}

fn placeholder_tool_result(
    task_id: &BackgroundProcessId,
    log_path: Option<&std::path::Path>,
) -> PublishedToolOutcome {
    // #1890：占位附任务日志文件路径——运行中即可主动查看进度
    //（优先等完成通知；确需中途进度用 BackgroundProcessLogs 增量查询
    // 或 Read/Bash 读取该文件）。
    let log_hint = log_path
        .map(|path| format!(" Progress log: {}", path.display()))
        .unwrap_or_default();
    PublishedToolOutcome::success_text(format!(
        "Running in the background ({}).{log_hint} Result will be delivered on completion.",
        task_id.as_str(),
    ))
}

async fn cancellation_outcome(
    declaration: CancellationDeclaration,
    mut join_handle: tokio::task::JoinHandle<PublishedToolOutcome>,
    grace: Duration,
    timed_out: bool,
) -> PublishedToolOutcome {
    if declaration == CancellationDeclaration::Cooperative
        && tokio::time::timeout(grace, &mut join_handle).await.is_ok()
    {
        return if timed_out {
            PublishedToolOutcome::timed_out(
                "tool reached effective deadline",
                CleanupConfirmation::Confirmed,
            )
        } else {
            PublishedToolOutcome::cancelled("tool cancelled by caller")
        };
    }
    PublishedToolOutcome::cancellation_unconfirmed(
        if timed_out {
            "tool reached effective deadline; cleanup unconfirmed"
        } else {
            "tool cancellation requested; cleanup unconfirmed"
        },
        vec!["tool may still have observable side effects".to_string()],
        Vec::new(),
    )
}

fn terminal_receipt(outcome: &PublishedToolOutcome) -> ToolTerminalReceiptData {
    match outcome {
        PublishedToolOutcome::TimedOut(details) => ToolTerminalReceiptData::new(
            context::ToolOutcomeKindData::TimedOut,
            details.safe_reason.clone(),
            receipt_cleanup(details.cleanup),
        ),
        PublishedToolOutcome::CancellationUnconfirmed(details) => {
            details.possible_side_effects.iter().fold(
                ToolTerminalReceiptData::new(
                    context::ToolOutcomeKindData::CancellationUnconfirmed,
                    details.safe_reason.clone(),
                    receipt_cleanup(details.cleanup),
                ),
                |receipt, effect| receipt.with_possible_side_effect(effect.clone()),
            )
        }
        PublishedToolOutcome::Cancelled(cancelled) => ToolTerminalReceiptData::new(
            context::ToolOutcomeKindData::Cancelled,
            cancelled.reason.clone(),
            ReceiptCleanupConfirmation::Confirmed,
        ),
        PublishedToolOutcome::Success(_) => ToolTerminalReceiptData::new(
            context::ToolOutcomeKindData::Success,
            "tool completed",
            ReceiptCleanupConfirmation::NotApplicable,
        ),
        PublishedToolOutcome::Failure(failure) => ToolTerminalReceiptData::new(
            context::ToolOutcomeKindData::Failure,
            failure.safe_message.clone(),
            ReceiptCleanupConfirmation::NotApplicable,
        ),
        PublishedToolOutcome::Suspended(_) => ToolTerminalReceiptData::new(
            context::ToolOutcomeKindData::Suspended,
            "tool suspended",
            ReceiptCleanupConfirmation::NotApplicable,
        ),
    }
}

fn receipt_cleanup(cleanup: CleanupConfirmation) -> ReceiptCleanupConfirmation {
    match cleanup {
        CleanupConfirmation::Confirmed => ReceiptCleanupConfirmation::Confirmed,
        CleanupConfirmation::Unconfirmed => ReceiptCleanupConfirmation::Unconfirmed,
        CleanupConfirmation::NotApplicable => ReceiptCleanupConfirmation::NotApplicable,
    }
}

pub(crate) fn earliest_deadline(
    scope: Option<SystemTime>,
    run: Option<SystemTime>,
    descriptor: Option<SystemTime>,
) -> Option<SystemTime> {
    [scope, run, descriptor].into_iter().flatten().min()
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ToolExecutionSupervisorError {
    #[error("Tool 不在当前 Catalog：{0}")]
    ToolUnavailable(String),
    #[error(transparent)]
    Receipt(#[from] context::ToolReceiptMutationError),
}

#[cfg(test)]
#[path = "execution_supervisor_tests.rs"]
mod tests;

/// #252 PR2：转后台调用的一句话摘要（监督器账本 display 用）。
fn invocation_summary_text(call: &SupervisedToolCall) -> String {
    format!(
        "tool={} input={}",
        call.identity.tool_name, call.input_preview
    )
}

/// #252 PR2：receipt 终态 → 后台进程终态映射。
fn terminal_kind_from_outcome(
    terminal: &ToolTerminalReceiptData,
) -> crate::domain::background_process::BackgroundProcessTerminalKind {
    use crate::domain::background_process::BackgroundProcessTerminalKind as Kind;
    match terminal.outcome {
        context::ToolOutcomeKindData::Success => Kind::Success,
        context::ToolOutcomeKindData::Failure => Kind::Failure,
        context::ToolOutcomeKindData::TimedOut
        | context::ToolOutcomeKindData::CancellationUnconfirmed => Kind::TimedOut,
        context::ToolOutcomeKindData::Cancelled
        | context::ToolOutcomeKindData::Suspended
        | context::ToolOutcomeKindData::Denied => Kind::Stopped,
    }
}

/// #252 PR2：终态安全文本（Success 首个文本块；其余用 safe_reason）。
fn terminal_output_text(outcome: &PublishedToolOutcome) -> Option<String> {
    match outcome {
        PublishedToolOutcome::Success(success) => {
            success.content.first().map(|block| block.text.clone())
        }
        PublishedToolOutcome::Failure(failure) => Some(failure.safe_message.clone()),
        PublishedToolOutcome::TimedOut(details) => Some(details.safe_reason.clone()),
        PublishedToolOutcome::CancellationUnconfirmed(details) => Some(details.safe_reason.clone()),
        PublishedToolOutcome::Cancelled(cancelled) => Some(cancelled.reason.clone()),
        PublishedToolOutcome::Suspended(_) => None,
    }
}
