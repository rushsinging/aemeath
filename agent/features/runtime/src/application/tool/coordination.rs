//! tool_coordination — Tool 调用编排：Policy/Hook/审批/并发/结果回收。
//!
//! 对应设计：`docs/design/02-modules/runtime/02-module-boundaries.md` §2。
//!
//! 本模块拥有 Main/Sub 共用的调用准备与稳定回收策略。UI 事件、进度流和
//! interaction waiter 仍由各自 adapter 处理；typed continuation 由 #878 收口。

use crate::application::hook::outcome_mapper::{RuntimeHookDirective, RuntimeHookReason};
use crate::application::loop_engine::chat::streaming_tool::StreamingToolRoundResult;
use crate::application::loop_engine::{
    ApprovalRequiredCall, LoopEngineError, SuspendedToolCall, ToolGuardDecision, ToolStep,
};
use crate::application::run::execution_state::RunExecutionState;
use crate::application::tool::agent::{ToolCall, ToolExecution};
use async_trait::async_trait;
use policy::{PolicyDecision, PolicyPort, PolicyRequest};
use std::collections::HashMap;
use std::path::Path;
use tokio_util::sync::CancellationToken;
use tools::{ToolCatalogSnapshot, ToolName};

pub(crate) struct ToolRoundContext<'a> {
    pub runtime_context: &'a crate::application::run::context::RuntimeContext,
    pub agent: crate::application::tool::agent::Agent,
    pub turn_context: crate::application::loop_engine::chat::RuntimeRunContext,
    pub language: &'a str,
    pub workspace_read: std::sync::Arc<dyn project::WorkspaceRead>,
    pub session_id: &'a str,
    pub materializer:
        &'a crate::application::tool::tool_result_materializer::ToolResultMaterializer,
    pub log_patch: logging::LogContextPatch,
}

#[derive(Debug)]
pub struct ToolRoundOutcome {
    pub step: crate::application::loop_engine::ToolStep,
    pub continuation: ToolRoundContinuation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolRoundContinuation {
    None,
    ToolResults,
}

pub(crate) struct ToolRoundCoordinator<'a, O> {
    context: ToolRoundContext<'a>,
    observer: O,
}

impl<'a, O> ToolRoundCoordinator<'a, O>
where
    O: ToolRoundObserver,
{
    pub(crate) fn new(context: ToolRoundContext<'a>, observer: O) -> Self {
        Self { context, observer }
    }

    pub(crate) async fn execute(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
        calls: &[(ToolCall, ToolGuardDecision)],
        cancel: &CancellationToken,
    ) -> Result<ToolRoundOutcome, crate::application::loop_engine::LoopEngineError> {
        logging::within(
            self.context.log_patch.clone(),
            execute_tools_impl(
                &self.context,
                &mut self.observer,
                execution,
                run_id,
                step_id,
                calls,
                cancel,
            ),
        )
        .await
    }

    /// #1494：汇总边流边执行的旁路结果（不执行工具，只统一收尾）。
    pub(crate) async fn finalize_streaming(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        step_id: &sdk::RunStepId,
        rounds: Vec<StreamingToolRoundResult>,
        cancel: &CancellationToken,
    ) -> Result<ToolRoundOutcome, crate::application::loop_engine::LoopEngineError> {
        logging::within(
            self.context.log_patch.clone(),
            finalize_streaming_rounds(self, execution, step_id, rounds, cancel),
        )
        .await
    }
}

#[async_trait]
pub(crate) trait ToolRoundObserver: Send {
    async fn execution_started(
        &mut self,
        _turn: usize,
        _all_calls: &[ToolCall],
        _executable: &[ToolCall],
    ) {
    }
    async fn execution_finished(
        &mut self,
        _execution: &crate::application::run::execution_state::RunExecutionState,
        _turn: usize,
        _results: &[ToolExecution],
    ) {
    }
    async fn cancelled_results_completed(&mut self, _results: &[ToolExecution]) {}
    async fn results_materialized(
        &mut self,
        _execution: &crate::application::run::execution_state::RunExecutionState,
    ) {
    }
    async fn round_finished(
        &mut self,
        _step_id: &sdk::RunStepId,
        _call_count: usize,
        _turn: usize,
        _cancel: &CancellationToken,
    ) {
    }
}

async fn execute_tools_impl<O: ToolRoundObserver>(
    context: &ToolRoundContext<'_>,
    observer: &mut O,
    execution: &mut crate::application::run::execution_state::RunExecutionState,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
    calls: &[(ToolCall, ToolGuardDecision)],
    cancel: &CancellationToken,
) -> Result<ToolRoundOutcome, crate::application::loop_engine::LoopEngineError> {
    use crate::application::loop_engine::ToolStep;
    if calls.is_empty() {
        return Ok(ToolRoundOutcome {
            step: ToolStep::Continue,
            continuation: ToolRoundContinuation::None,
        });
    }
    let raw_calls: Vec<_> = calls.iter().map(|(call, _)| call.clone()).collect();
    let workspace_root = context.workspace_read.current_workspace_root();
    let agent = &context.agent;
    let executable = prepare_tool_round(
        calls,
        &agent.catalog,
        context.runtime_context.policy_ref().as_ref(),
        run_id,
        step_id,
        &workspace_root,
    )
    .executable
    .into_iter()
    .map(|call| call.call)
    .collect::<Vec<_>>();
    observer
        .execution_started(execution.step_count(), &raw_calls, &executable)
        .await;
    let sink = context.runtime_context.event_sink();
    let round = crate::application::loop_engine::chat::tools::execute_tool_round(
        &context.turn_context,
        &raw_calls,
        &agent.catalog,
        context.runtime_context.policy_ref().as_ref(),
        run_id,
        step_id,
        agent,
        &sink,
        context.runtime_context.hooks_ref(),
        context.runtime_context.activities().as_ref(),
        cancel,
        context.language,
        &context.workspace_read,
        calls,
    )
    .await;
    observer
        .execution_finished(execution, execution.step_count(), &round.results)
        .await;
    let interaction_ids: std::collections::HashSet<_> = round
        .suspensions
        .iter()
        .map(|s| s.call.id.clone())
        .chain(round.approvals.iter().map(|a| a.call.id.clone()))
        .collect();
    let selected = if interaction_ids.is_empty() {
        round.results.clone()
    } else {
        round
            .results
            .iter()
            .filter(|result| !interaction_ids.contains(&result.call_id))
            .cloned()
            .collect()
    };
    let cancelled = cancel.is_cancelled();
    let results = if cancelled && interaction_ids.is_empty() {
        let convergence = complete_cancelled_tool_round(&raw_calls, selected);
        observer
            .cancelled_results_completed(&convergence.results)
            .await;
        convergence.results
    } else {
        selected
    };
    let completed_results = calls
        .iter()
        .filter(|(call, _)| !interaction_ids.contains(&call.id))
        .map(|(call, decision)| {
            let bypassed = round.fuse_bypassed.contains(&call.id);
            let status = if matches!(decision, ToolGuardDecision::Allow) || bypassed {
                crate::domain::agent_run::ToolCallStatus::Success
            } else {
                crate::domain::agent_run::ToolCallStatus::Cancelled
            };
            (call.id.clone(), status)
        })
        .collect();
    finalize_tool_round_results(
        context,
        observer,
        execution,
        step_id,
        results,
        round.suspensions,
        round.approvals,
        round.fuse_bypassed,
        completed_results,
        &agent.runtime_cancellation,
        cancel,
    )
    .await
}

/// #1494：边流边执行结果汇总——旁路轮次已执行完工具，这里只做统一收尾：
/// materialize + 写入消息历史 + 状态登记 + suspensions / approvals 路由。
/// 与普通工具轮次 `execute_tools_impl` 尾部共享同一收尾逻辑。
pub(crate) async fn finalize_streaming_rounds<O: ToolRoundObserver>(
    coordinator: &mut ToolRoundCoordinator<'_, O>,
    execution: &mut RunExecutionState,
    step_id: &sdk::RunStepId,
    rounds: Vec<StreamingToolRoundResult>,
    cancel: &CancellationToken,
) -> Result<ToolRoundOutcome, LoopEngineError> {
    let mut results = Vec::new();
    let mut suspensions = Vec::new();
    let mut approvals = Vec::new();
    let mut fuse_bypassed = Vec::new();
    for round in rounds {
        results.extend(round.results);
        suspensions.extend(round.suspensions);
        approvals.extend(round.approvals);
        fuse_bypassed.extend(round.fuse_bypassed);
    }
    let interaction_ids: std::collections::HashSet<_> = suspensions
        .iter()
        .map(|s| s.call.id.clone())
        .chain(approvals.iter().map(|a| a.call.id.clone()))
        .collect();
    let selected: Vec<ToolExecution> = results
        .iter()
        .filter(|result| !interaction_ids.contains(&result.call_id))
        .cloned()
        .collect();
    let results = if cancel.is_cancelled() && interaction_ids.is_empty() {
        let calls = selected
            .iter()
            .enumerate()
            .map(|(index, result)| ToolCall {
                id: result.call_id.clone(),
                provider_id: result.provider_id.clone(),
                name: result.tool_name.clone(),
                index,
                input: serde_json::Value::Null,
            })
            .collect::<Vec<_>>();
        let convergence = converge_cancelled_tool_round(&calls, selected);
        coordinator
            .observer
            .cancelled_results_completed(&convergence.results)
            .await;
        convergence.results
    } else {
        selected
    };
    let completed_results: Vec<_> = results
        .iter()
        .map(|result| {
            (
                result.call_id.clone(),
                crate::domain::agent_run::ToolCallStatus::Success,
            )
        })
        .collect();
    finalize_tool_round_results(
        &coordinator.context,
        &mut coordinator.observer,
        execution,
        step_id,
        results,
        suspensions,
        approvals,
        fuse_bypassed,
        completed_results,
        &coordinator.context.agent.runtime_cancellation,
        cancel,
    )
    .await
}

/// 工具轮次统一收尾（普通轮次与 #1494 旁路汇总共用）：
/// materialize 结果 → 写入消息历史 → 状态登记 → suspensions / approvals 路由。
#[allow(clippy::too_many_arguments)]
async fn finalize_tool_round_results<O: ToolRoundObserver>(
    context: &ToolRoundContext<'_>,
    observer: &mut O,
    execution: &mut RunExecutionState,
    step_id: &sdk::RunStepId,
    results: Vec<ToolExecution>,
    suspensions: Vec<SuspendedToolCall>,
    approvals: Vec<ApprovalRequiredCall>,
    fuse_bypassed: Vec<sdk::ToolCallId>,
    completed_results: Vec<(sdk::ToolCallId, crate::domain::agent_run::ToolCallStatus)>,
    run_cancel: &CancellationToken,
    cancel: &CancellationToken,
) -> Result<ToolRoundOutcome, LoopEngineError> {
    let result_count = results.len();
    if !results.is_empty() {
        let message = crate::application::loop_engine::shared::materialize_tool_results(
            context.materializer,
            results,
            context.session_id,
        )
        .await;
        execution.append_message(message.clone());
        execution.record_step_message(message);
        observer.results_materialized(execution).await;
    }
    if cancel.is_cancelled() {
        return Err(crate::application::loop_engine::LoopEngineError::Cancelled);
    }
    if !suspensions.is_empty() {
        return Ok(ToolRoundOutcome {
            step: ToolStep::InteractionSuspended {
                suspended: suspensions,
                completed_results,
                fuse_bypassed,
            },
            continuation: ToolRoundContinuation::None,
        });
    }
    if !approvals.is_empty() {
        return Ok(ToolRoundOutcome {
            step: ToolStep::AwaitingToolApproval {
                calls_needing_approval: approvals,
                completed_results,
                fuse_bypassed,
            },
            continuation: ToolRoundContinuation::None,
        });
    }
    observer
        .round_finished(step_id, result_count, execution.step_count(), run_cancel)
        .await;
    Ok(ToolRoundOutcome {
        step: crate::application::loop_engine::tool_strategy::step_from_fuse_bypass(fuse_bypassed),
        continuation: ToolRoundContinuation::ToolResults,
    })
}

pub(crate) mod identity;
pub(crate) mod loop_guard;

#[derive(Clone)]
pub(crate) struct DeniedToolCall {
    pub call: ToolCall,
    pub reason: String,
}

pub(crate) struct PreparedToolCall {
    pub call: ToolCall,
    pub authorization: tools::AuthorizationContext,
}

#[derive(Default)]
pub(crate) struct PreparedToolRound {
    pub executable: Vec<PreparedToolCall>,
    pub guard_blocked: Vec<ToolExecution>,
    pub denied: Vec<DeniedToolCall>,
    pub fuse_bypassed: Vec<sdk::ToolCallId>,
    /// #1248 Task 5: Calls that Policy marked RequireApproval. The engine
    /// reads this to create [`ToolApproval`] interaction intents instead of
    /// denying them inline.  On approve, only this specific call is executed
    /// with its original authorization; on deny, a typed denied result.
    pub require_approval: Vec<RequireApprovalCall>,
}

/// #1248 Task 5: A tool call that needs approval before execution.
#[derive(Clone)]
pub(crate) struct RequireApprovalCall {
    pub call: ToolCall,
    pub authorization: tools::AuthorizationContext,
    pub reason: String,
    pub subject: String,
}

/// Applies catalog validity, Policy and Runtime guard in canonical order.
///
/// Calls absent from the frozen catalog are denied before Policy because no
/// trustworthy capability set exists. Policy is evaluated once per valid call;
/// its AuthorizationContext decides whether the Runtime fuse remains active.
pub(crate) fn prepare_tool_round(
    calls: &[(ToolCall, ToolGuardDecision)],
    catalog: &ToolCatalogSnapshot,
    policy: &dyn PolicyPort,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
    workspace_root: &Path,
) -> PreparedToolRound {
    let mut prepared = PreparedToolRound::default();
    for (call, decision) in calls {
        let Some(descriptor) = catalog.find(&ToolName::new(&call.name)) else {
            prepared.denied.push(DeniedToolCall {
                call: call.clone(),
                reason: "Tool is not present in the catalog".to_string(),
            });
            continue;
        };
        let request = match PolicyRequest::new(
            run_id.clone(),
            step_id.clone(),
            ToolName::new(&call.name),
            descriptor.required_capabilities,
            workspace_root,
        ) {
            Ok(request) => request,
            Err(error) => {
                prepared.denied.push(DeniedToolCall {
                    call: call.clone(),
                    reason: error.to_string(),
                });
                continue;
            }
        };
        match policy.evaluate(&request) {
            PolicyDecision::Allow(authorization) => {
                if let ToolGuardDecision::SoftBlock { reason } = decision {
                    if authorization.enforce_tool_fuse {
                        prepared
                            .guard_blocked
                            .push(blocked_tool_execution(call, reason));
                        continue;
                    }
                    prepared.fuse_bypassed.push(call.id.clone());
                }
                prepared.executable.push(PreparedToolCall {
                    call: call.clone(),
                    authorization,
                });
            }
            PolicyDecision::Deny { reason } => prepared.denied.push(DeniedToolCall {
                call: call.clone(),
                reason: format!("{reason:?}"),
            }),
            PolicyDecision::RequireApproval { reason, subject } => {
                // #1248 Task 5: Surface RequireApproval for the engine
                // to create ToolApproval interaction intents. No longer
                // deny inline.
                prepared.require_approval.push(RequireApprovalCall {
                    call: call.clone(),
                    authorization: tools::AuthorizationContext::STANDARD,
                    reason: format!("{reason:?}"),
                    subject: format!("{subject:?}"),
                });
            }
        }
    }
    prepared
}

#[derive(Debug)]
pub struct CancelledToolRoundConvergence {
    pub results: Vec<ToolExecution>,
}

pub(crate) fn converge_cancelled_tool_round(
    calls: &[ToolCall],
    results: Vec<ToolExecution>,
) -> CancelledToolRoundConvergence {
    let mut by_id: HashMap<_, _> = results
        .into_iter()
        .map(|result| (result.call_id.clone(), result))
        .collect();
    let results = calls
        .iter()
        .map(|call| {
            by_id.remove(&call.id).unwrap_or_else(|| {
                ToolExecution::new_typed(
                    call,
                    tools::ToolExecutionOutcome::cancelled("Command cancelled by user"),
                )
            })
        })
        .collect::<Vec<_>>();
    for result in &results {
        debug_assert!(matches!(
            result.typed_outcome,
            tools::ToolExecutionOutcome::Success(_)
                | tools::ToolExecutionOutcome::Failure(_)
                | tools::ToolExecutionOutcome::Cancelled(_)
                | tools::ToolExecutionOutcome::TimedOut(_)
                | tools::ToolExecutionOutcome::CancellationUnconfirmed(_)
                | tools::ToolExecutionOutcome::Suspended(_)
        ));
    }
    CancelledToolRoundConvergence { results }
}

pub(crate) fn complete_cancelled_tool_round(
    calls: &[ToolCall],
    results: Vec<ToolExecution>,
) -> CancelledToolRoundConvergence {
    converge_cancelled_tool_round(calls, results)
}

/// Restores original model call order after concurrent execution and gate paths.
pub(crate) fn restore_tool_call_order(
    calls: &[ToolCall],
    results: Vec<ToolExecution>,
) -> Vec<ToolExecution> {
    let mut by_id: HashMap<_, _> = results
        .into_iter()
        .map(|result| (result.call_id.clone(), result))
        .collect();
    calls
        .iter()
        .filter_map(|call| by_id.remove(&call.id))
        .collect()
}

pub(crate) fn blocked_tool_execution(call: &ToolCall, reason: &str) -> ToolExecution {
    let message = format!(
        "Tool call blocked: repeated tool-call loop detected.\n\nReason: {reason}\n\nDo not call this tool again with the same inputs. Use the existing results to summarize findings, change strategy, or ask the user for clarification."
    );
    ToolExecution::new(
        call,
        tools::ToolOutcome {
            text: message.clone(),
            data: serde_json::json!({
                "status": "error",
                "message": message,
                "reason": reason,
                "error_type": "tool_call_loop_fuse",
            }),
            is_error: true,
            images: Vec::new(),
            task_change: None,
        },
    )
}

// ─── Hook directive application ───────────────────────────────

/// Structured outcome of applying a [`RuntimeHookDirective`] to a single
/// [`ToolCall`].
///
/// The caller is expected to match on the variant to decide the next step
/// (execute, error-synthesize, request approval, or block).
#[derive(Clone)]
pub enum HookDirectiveOutcome {
    /// Tool call is ready to execute with validated, policy-cleared input.
    ///
    /// The `call` carries the **updated** input (from `UpdatedInput` /
    /// `ContextAndInput`). `context` is `Some` only when the directive was
    /// `ContextAndInput`, preserving the hook-injected guidance for the caller.
    Ready {
        /// The call with validated, updated input.
        call: ToolCall,
        /// Authorization returned by the mandatory post-update Policy evaluation.
        authorization: tools::AuthorizationContext,
        /// Context string from `ContextAndInput` (preserved for caller injection).
        context: Option<String>,
    },
    /// Continue with the original call unchanged.
    ///
    /// Produced by `Continue` and `Context` directives. `context` is `Some`
    /// only when the directive was `Context`.
    Continue {
        /// The original, unmodified call.
        call: ToolCall,
        /// Context string from `Context` (preserved for caller injection).
        context: Option<String>,
    },
    /// Updated input failed JSON Schema validation against the frozen catalog
    /// descriptor.
    InvalidInput {
        /// The original call (updated input is discarded).
        call: ToolCall,
        /// Human-readable validation error message.
        error: String,
    },
    /// Policy denied the tool call after re-evaluation with the updated input.
    Denied {
        /// The original call.
        call: ToolCall,
        /// Denial reason.
        reason: String,
    },
    /// Policy requires approval before the updated input may execute.
    ApprovalRequired {
        /// The call with validated, updated input.
        call: ToolCall,
        /// Approval reason.
        reason: String,
    },
    /// Hook explicitly blocked the call.
    Blocked {
        /// The original call.
        call: ToolCall,
        /// Structured block reason from the hook.
        reason: RuntimeHookReason,
    },
}

impl std::fmt::Debug for HookDirectiveOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ready {
                call,
                context,
                authorization,
            } => f
                .debug_struct("Ready")
                .field("call_name", &call.name)
                .field("call_index", &call.index)
                .field("context", context)
                .field("authorization", authorization)
                .finish(),
            Self::Continue { call, context } => f
                .debug_struct("Continue")
                .field("call_name", &call.name)
                .field("call_index", &call.index)
                .field("context", context)
                .finish(),
            Self::InvalidInput { call, error } => f
                .debug_struct("InvalidInput")
                .field("call_name", &call.name)
                .field("call_index", &call.index)
                .field("error", error)
                .finish(),
            Self::Denied { call, reason } => f
                .debug_struct("Denied")
                .field("call_name", &call.name)
                .field("call_index", &call.index)
                .field("reason", reason)
                .finish(),
            Self::ApprovalRequired { call, reason } => f
                .debug_struct("ApprovalRequired")
                .field("call_name", &call.name)
                .field("call_index", &call.index)
                .field("reason", reason)
                .finish(),
            Self::Blocked { call, reason } => f
                .debug_struct("Blocked")
                .field("call_name", &call.name)
                .field("call_index", &call.index)
                .field("reason", reason)
                .finish(),
        }
    }
}

/// Applies a [`RuntimeHookDirective`] to a single [`ToolCall`] and returns a
/// structured [`HookDirectiveOutcome`].
///
/// For directives that update the input (`UpdatedInput` / `ContextAndInput`),
/// the function performs the canonical re-validation sequence:
///
/// 1. Look up the frozen catalog descriptor by tool name.
/// 2. Validate the updated input against the descriptor's `input_schema` via
///    [`tools::validate_tool_input`].
/// 3. Rebuild a [`PolicyRequest`] using the descriptor's `required_capabilities`.
/// 4. Re-evaluate policy.
///
/// Non-mutating directives (`Continue`, `Context`) short-circuit to
/// [`HookDirectiveOutcome::Continue`] without touching the catalog or policy.
/// `Block` maps directly to [`HookDirectiveOutcome::Blocked`].
///
/// This function does **not** call `ToolExecutionPort` — it only decides *what*
/// to do; the caller performs the actual execution.
pub fn apply_hook_directive_to_tool_call(
    call: &ToolCall,
    directive: RuntimeHookDirective,
    catalog: &ToolCatalogSnapshot,
    policy: &dyn PolicyPort,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
    workspace_root: &Path,
) -> HookDirectiveOutcome {
    match directive {
        RuntimeHookDirective::Continue => HookDirectiveOutcome::Continue {
            call: call.clone(),
            context: None,
        },
        RuntimeHookDirective::Context { context } => HookDirectiveOutcome::Continue {
            call: call.clone(),
            context: Some(context),
        },
        RuntimeHookDirective::Block { reason } => HookDirectiveOutcome::Blocked {
            call: call.clone(),
            reason,
        },
        RuntimeHookDirective::UpdatedInput { input } => revalidate_updated_input(
            call,
            &input,
            None,
            catalog,
            policy,
            run_id,
            step_id,
            workspace_root,
        ),
        RuntimeHookDirective::ContextAndInput { context, input } => revalidate_updated_input(
            call,
            &input,
            Some(context),
            catalog,
            policy,
            run_id,
            step_id,
            workspace_root,
        ),
    }
}

/// Re-validates updated input and re-evaluates policy, returning the
/// appropriate [`HookDirectiveOutcome`].
fn revalidate_updated_input(
    call: &ToolCall,
    input: &serde_json::Value,
    context: Option<String>,
    catalog: &ToolCatalogSnapshot,
    policy: &dyn PolicyPort,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
    workspace_root: &Path,
) -> HookDirectiveOutcome {
    // 1. Look up the frozen catalog descriptor.
    let Some(descriptor) = catalog.find(&ToolName::new(&call.name)) else {
        return HookDirectiveOutcome::Denied {
            call: call.clone(),
            reason: "Tool is not present in the catalog".to_string(),
        };
    };

    // 2. Validate updated input against the descriptor's JSON Schema.
    if let Err(mismatch) = tools::validate_tool_input(&call.name, &descriptor.input_schema, input) {
        return HookDirectiveOutcome::InvalidInput {
            call: call.clone(),
            error: tools::format_tool_input_error(&mismatch),
        };
    }

    // 3. Rebuild PolicyRequest with the descriptor's required capabilities.
    let request = match PolicyRequest::new(
        run_id.clone(),
        step_id.clone(),
        ToolName::new(&call.name),
        descriptor.required_capabilities,
        workspace_root,
    ) {
        Ok(request) => request,
        Err(error) => {
            return HookDirectiveOutcome::Denied {
                call: call.clone(),
                reason: error.to_string(),
            };
        }
    };

    // 4. Re-evaluate policy against the rebuilt request.
    let updated_call = ToolCall {
        input: input.clone(),
        ..call.clone()
    };
    match policy.evaluate(&request) {
        PolicyDecision::Allow(authorization) => HookDirectiveOutcome::Ready {
            call: updated_call,
            context,
            authorization,
        },
        PolicyDecision::Deny { reason } => HookDirectiveOutcome::Denied {
            call: call.clone(),
            reason: format!("{reason:?}"),
        },
        PolicyDecision::RequireApproval { reason, subject } => {
            HookDirectiveOutcome::ApprovalRequired {
                call: updated_call,
                reason: format!("approval required: {subject:?}: {reason:?}"),
            }
        }
    }
}

#[cfg(test)]
#[path = "coordination_tests.rs"]
mod tests;
