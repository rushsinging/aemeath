//! Hook dispatch adapter —— 按 point 匹配 subscription、串行执行、重试与聚合。
//!
//! 对应设计：`docs/design/02-modules/hook/README.md` §4 §6 §10 与
//! `01-run-loop-integration.md` §1 §2。
//!
//! - `Dispatcher` 实现 `HookPort`：matcher 过滤 / order 排序 / Block 短路 /
//!   Context 合并 / UpdatedInput 串联 / ExecutionFailed 重试 / StopFailure 派发；
//! - 私有 [`executor::Executor`] port 抽象「单次命令执行」，便于测试用
//!   [`fake::Scripted`] fake 替代真实进程；
//! - 生产构造入口 [`Dispatcher::try_new`] 内部装配
//!   [`executor::ProcessDriverExecutor`]（`adapters/process.rs` 的 `ProcessDriver`
//!   适配），不对外泄漏执行器技术类型。
//!
//! 本文件只含**编排逻辑**（匹配 / 排序 / 短路 / 合并 / 重试 / 聚合 / StopFailure）；
//! `Executor` port 与生产适配位于 [`executor`]，测试 fake 位于 [`fake`]。
//!
//! `Executor` / `RawExecution` / `ExecutionFault` / `ProcessDriverExecutor` 均为
//! 适配器 detail（`pub(crate)`），**NEVER** 进入 crate 稳定 façade。

mod executor;
#[cfg(test)]
mod fake;
mod helpers;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::time::Instant;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::domain::invocation::{HookInvocation, HookPoint, StopFailureInput};
use crate::domain::outcome::{
    HookDirective, HookDisplayMessage, HookDisplayMessageKind, HookExecution, HookExecutionStatus,
    HookOutcome,
};
use crate::domain::protocol::classify_output;
use crate::domain::subscription::{HookCommand, HookSubscription, SubscriptionError};

use crate::ports::{HookDispatchContext, HookPort};

pub(crate) use executor::{ExecutionFault, Executor, ProcessDriverExecutor};
#[cfg(test)]
use fake::{ScriptStep, Scripted};
use helpers::{
    classify_error_summary, last_error_of, matcher_hits, matcher_source, push_context,
    synthesize_cancelled_directive, synthesize_exhausted_directive,
};

/// 单 Hook 执行重试上限（含第一次）。
///
/// 设计 §6：`hook.max_attempts = 3`。任意 ExecutionFailed（spawn / wait / IO /
/// timeout / 非法 JSON / 能力矩阵违规）连续发生时最多重试到本上限；业务 Block
/// （非零 exit / JSON `decision:block` / `continue:false`）不重试。
pub const MAX_ATTEMPTS: u8 = 3;

// ════════════════════════════════════════════════════════════
// Dispatcher
// ════════════════════════════════════════════════════════════

/// Hook dispatcher：按 point 匹配 subscription、串行执行、重试与聚合。
///
/// 内部以 `Box<dyn Executor>` 持有执行端口，对外只暴露稳定构造入口
/// [`Dispatcher::try_new`]（cwd + env 白名单装配受管子进程执行器），
/// 不泄漏 `Executor` / `RawExecution` / `ExecutionFault` 等技术类型。
pub struct Dispatcher {
    subscriptions: Vec<HookSubscription>,
    executor: Box<dyn Executor>,
}

impl Dispatcher {
    /// 生产严格构造：Hook adapter 装配受管子进程执行器。
    ///
    /// cwd 是每次 dispatch 的 invocation context，不得在 Dispatcher 构造时冻结。
    /// 任一 subscription 配置非法（如 Stop 配 failure_policy、非前置闸门配 Block）
    /// 即返回全部错误——与设计 §4「非法组合在 Config 校验阶段拒绝，而非运行时
    /// 静默忽略」一致。**NEVER** 静默丢弃非法 subscription。
    pub fn try_new(
        subscriptions: Vec<HookSubscription>,
        env: HashMap<String, String>,
    ) -> Result<Self, Vec<SubscriptionError>> {
        Self::build(subscriptions, Box::new(ProcessDriverExecutor::new(env)))
    }

    /// 共用装配：严格校验全部 subscription 后装配执行器。
    fn build(
        subscriptions: Vec<HookSubscription>,
        executor: Box<dyn Executor>,
    ) -> Result<Self, Vec<SubscriptionError>> {
        let mut errors = Vec::new();
        for sub in &subscriptions {
            if let Err(err) = sub.validate() {
                errors.push(err);
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(Self {
            subscriptions,
            executor,
        })
    }

    /// 测试专用：注入脚本化执行器，subscription 必须全部合法（否则 panic）。
    #[cfg(test)]
    fn with_scripted(subscriptions: Vec<HookSubscription>, executor: Scripted) -> Self {
        Self::build(subscriptions, Box::new(executor))
            .expect("测试用 HookSubscription 必须全部合法")
    }
}

/// 单次 subscription 调度的内部结果。
enum AttemptOutcome {
    /// subscription 成功返回 directive（含业务 Block，业务 Block 不重试）。
    ///
    /// `executions` 携带该 subscription 的**全部** attempt 明细（含此前失败的
    /// 尝试与最终成功的尝试），确保 `HookOutcome.executions` 完整保留重试轨迹。
    /// `system_message` 为本次成功执行独立保留的 systemMessage（展示用）。
    Success {
        executions: Vec<HookExecution>,
        directive: HookDirective,
        system_message: Option<String>,
    },
    /// 重试耗尽（ExecutionFailed 达到 MAX_ATTEMPTS）。
    Exhausted { executions: Vec<HookExecution> },
    /// 被 cancellation 终止（不重试，但仍保留这一次 attempt 的 ExecutionFailed 明细）。
    Cancelled { executions: Vec<HookExecution> },
}

#[async_trait]
impl HookPort for Dispatcher {
    async fn dispatch(
        &self,
        invocation: HookInvocation,
        cancellation: &CancellationToken,
    ) -> HookOutcome {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        self.dispatch_at(invocation, HookDispatchContext::new(cwd), cancellation)
            .await
    }

    async fn dispatch_at(
        &self,
        invocation: HookInvocation,
        context: HookDispatchContext,
        cancellation: &CancellationToken,
    ) -> HookOutcome {
        let point = invocation.point();

        // matcher 过滤 + order + 声明顺序（sort_by_key 稳定，同 order 按声明顺序）。
        let mut matching: Vec<&HookSubscription> = self
            .subscriptions
            .iter()
            .filter(|s| s.enabled && s.point == point && matcher_hits(&s.matcher, &invocation))
            .collect();
        matching.sort_by_key(|s| s.order);

        // 当前调用（随 UpdatedInput 串联更新 payload 字段，再重新序列化给下一条 subscription）。
        let mut current_invocation = invocation;

        // 聚合状态：多个 ContinueWithContext 的 context 按顺序拼接；
        // 最近一次 UpdatedInput 的 input 作为最终 UpdatedInput。
        let mut aggregated_context: Option<String> = None;
        let mut final_input: Option<serde_json::Value> = None;
        let mut all_executions: Vec<HookExecution> = Vec::new();
        // BC 保留展示消息：按 executions 聚合顺序逐条保留 additionalContext / systemMessage。
        let mut messages: Vec<HookDisplayMessage> = Vec::new();

        for sub in matching {
            let current_input =
                serde_json::to_value(&current_invocation).unwrap_or(serde_json::json!({}));
            let invocation_env =
                invocation_environment(&current_invocation, context.cwd(), context.env());
            let outcome = self
                .execute_subscription(
                    sub,
                    &current_input,
                    context.cwd(),
                    &invocation_env,
                    cancellation,
                )
                .await;

            match outcome {
                AttemptOutcome::Success {
                    executions,
                    directive,
                    system_message,
                } => {
                    all_executions.extend(executions);
                    // 产生 directive / system_message 的成功 execution 是该 subscription
                    // executions 的最后一条；其聚合位置即展示消息的 execution_ordinal。
                    let execution_ordinal = all_executions.len() as u32;
                    let attempt = all_executions
                        .last()
                        .expect("Success 携带至少 1 条 execution")
                        .attempts;
                    let source = matcher_source(&sub.matcher);
                    // systemMessage 独立保留为展示消息（不折叠进 directive）。
                    if let Some(text) = system_message {
                        messages.push(HookDisplayMessage {
                            point,
                            source: source.clone(),
                            execution_ordinal,
                            attempt,
                            kind: HookDisplayMessageKind::SystemMessage,
                            text,
                        });
                    }
                    match directive {
                        HookDirective::Continue => {}
                        HookDirective::Block { reason } => {
                            return HookOutcome {
                                executions: all_executions,
                                directive: HookDirective::Block { reason },
                                messages,
                            };
                        }
                        HookDirective::ContinueWithContext { context } => {
                            messages.push(HookDisplayMessage {
                                point,
                                source: source.clone(),
                                execution_ordinal,
                                attempt,
                                kind: HookDisplayMessageKind::AdditionalContext,
                                text: context.clone(),
                            });
                            push_context(&mut aggregated_context, context);
                        }
                        HookDirective::ContinueWithUpdatedInput { input } => {
                            current_invocation.apply_updated_input(&input);
                            final_input = Some(input);
                        }
                        HookDirective::ContinueWithContextAndInput { context, input } => {
                            messages.push(HookDisplayMessage {
                                point,
                                source: source.clone(),
                                execution_ordinal,
                                attempt,
                                kind: HookDisplayMessageKind::AdditionalContext,
                                text: context.clone(),
                            });
                            push_context(&mut aggregated_context, context);
                            current_invocation.apply_updated_input(&input);
                            final_input = Some(input);
                        }
                    }
                }
                AttemptOutcome::Exhausted { executions } => {
                    all_executions.extend(executions);
                    let directive =
                        synthesize_exhausted_directive(point, sub.failure_policy, &all_executions);
                    // Block 短路：Stop（固定 Block，用户不可覆盖）或配置
                    // failure_policy=Block 的前置闸门。后续 subscription 不再执行。
                    if let HookDirective::Block { .. } = directive {
                        // Stop point 耗尽后尽力派发一次 StopFailure（不递归），
                        // 其执行明细并入原 Stop HookOutcome.executions。
                        if point == HookPoint::Stop {
                            let error = last_error_of(&all_executions).unwrap_or_default();
                            let sf_outcome = self
                                .dispatch_stop_failure(
                                    &current_invocation,
                                    error,
                                    context.cwd(),
                                    context.env(),
                                    cancellation,
                                )
                                .await;
                            all_executions.extend(sf_outcome.executions);
                        }
                        return HookOutcome {
                            executions: all_executions,
                            directive,
                            messages,
                        };
                    }
                    // 默认 / Continue policy：重试耗尽后不阻断流程。
                    // 已耗尽的 ExecutionFailed 明细保留在 all_executions 中，
                    // 继续执行后续 subscription 并聚合其成功 directive。
                }
                AttemptOutcome::Cancelled { executions } => {
                    // Cancelled 同样是一次 attempt：保留 ExecutionFailed 明细后再合成 directive。
                    all_executions.extend(executions);
                    let directive = synthesize_cancelled_directive(point, sub.failure_policy);
                    return HookOutcome {
                        executions: all_executions,
                        directive,
                        messages,
                    };
                }
            }
        }

        // 全部 subscription 完成 —— 组装聚合 directive。
        let directive = match (aggregated_context, final_input) {
            (Some(ctx), Some(inp)) => HookDirective::ContinueWithContextAndInput {
                context: ctx,
                input: inp,
            },
            (Some(ctx), None) => HookDirective::ContinueWithContext { context: ctx },
            (None, Some(inp)) => HookDirective::ContinueWithUpdatedInput { input: inp },
            (None, None) => HookDirective::Continue,
        };
        HookOutcome {
            executions: all_executions,
            directive,
            messages,
        }
    }
}

fn invocation_environment(
    invocation: &HookInvocation,
    cwd: &std::path::Path,
    base: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut env = base.clone();
    env.insert(
        "AEMEATH_HOOK_EVENT".to_string(),
        serde_json::to_string(&invocation.point()).unwrap_or_default(),
    );
    env.insert("AEMEATH_PROJECT_DIR".to_string(), cwd.display().to_string());
    env.insert("CLAUDE_PROJECT_DIR".to_string(), cwd.display().to_string());
    match invocation {
        HookInvocation::PreToolUse(input) => {
            env.insert("AEMEATH_TOOL_NAME".to_string(), input.tool_name.clone());
            env.insert(
                "AEMEATH_TOOL_INPUT".to_string(),
                input.tool_input.to_string(),
            );
        }
        HookInvocation::PostToolUse(input) => {
            env.insert("AEMEATH_TOOL_NAME".to_string(), input.tool_name.clone());
            env.insert(
                "AEMEATH_TOOL_INPUT".to_string(),
                input.tool_input.to_string(),
            );
            env.insert("AEMEATH_TOOL_OUTPUT".to_string(), input.tool_output.clone());
            env.insert(
                "AEMEATH_TOOL_IS_ERROR".to_string(),
                input.is_error.to_string(),
            );
        }
        HookInvocation::PostToolUseFailure(input) => {
            env.insert("AEMEATH_TOOL_NAME".to_string(), input.tool_name.clone());
            env.insert(
                "AEMEATH_TOOL_INPUT".to_string(),
                input.tool_input.to_string(),
            );
            env.insert("AEMEATH_TOOL_OUTPUT".to_string(), input.error.clone());
            env.insert("AEMEATH_TOOL_IS_ERROR".to_string(), "true".to_string());
        }
        HookInvocation::Stop(input) => {
            env.insert("AEMEATH_STOP_TURNS".to_string(), input.turns.to_string());
        }
        HookInvocation::PermissionRequest(input) | HookInvocation::PermissionDenied(input) => {
            env.insert(
                "AEMEATH_PERMISSION_TOOL_NAME".to_string(),
                input.tool_name.clone(),
            );
            env.insert(
                "AEMEATH_PERMISSION_RULE".to_string(),
                input.permission_rule.clone(),
            );
        }
        HookInvocation::InstructionsLoaded(input) => {
            env.insert(
                "AEMEATH_INSTRUCTIONS_FILE_PATH".to_string(),
                input.file_path.clone(),
            );
            env.insert(
                "AEMEATH_INSTRUCTIONS_TYPE".to_string(),
                input.instruction_type.clone(),
            );
        }
        HookInvocation::Notification(input) => {
            env.insert(
                "AEMEATH_NOTIFICATION_TEXT".to_string(),
                input.notification_text.clone(),
            );
            env.insert(
                "AEMEATH_NOTIFICATION_TYPE".to_string(),
                input.notification_type.clone(),
            );
        }
        _ => {}
    }
    env
}

impl Dispatcher {
    async fn execute_subscription(
        &self,
        sub: &HookSubscription,
        current_input: &serde_json::Value,
        cwd: &std::path::Path,
        env: &HashMap<String, String>,
        cancellation: &CancellationToken,
    ) -> AttemptOutcome {
        let mut attempts: u8 = 0;
        let mut executions: Vec<HookExecution> = Vec::new();
        let cwd_text = cwd.display().to_string();
        let command = HookCommand::new(
            sub.command
                .command
                .replace("{AEMEATH_PROJECT_DIR}", &cwd_text)
                .replace("{CLAUDE_PROJECT_DIR}", &cwd_text),
        );
        loop {
            attempts += 1;
            let start = Instant::now();
            let result = self
                .executor
                .execute(&command, current_input, cwd, env, sub.timeout, cancellation)
                .await;
            let duration = start.elapsed();

            match result {
                Ok(raw) => {
                    match classify_output(sub.point, raw.exit_code, &raw.stdout, &raw.stderr) {
                        Ok((directive, system_message)) => {
                            let status = match &directive {
                                HookDirective::Block { .. } => HookExecutionStatus::Blocked,
                                _ => HookExecutionStatus::Success,
                            };
                            let execution = HookExecution {
                                status,
                                attempts,
                                exit_code: raw.exit_code,
                                stdout: raw.stdout,
                                stderr: raw.stderr,
                                duration,
                            };
                            // 成功也必须保留 prior executions（此前失败的 attempt 明细），
                            // 使 HookOutcome.executions 完整反映全部重试轨迹。
                            executions.push(execution);
                            return AttemptOutcome::Success {
                                executions,
                                directive,
                                system_message,
                            };
                        }
                        Err(err) => {
                            let error = classify_error_summary(&err);
                            let execution = HookExecution {
                                status: HookExecutionStatus::ExecutionFailed {
                                    error: error.clone(),
                                },
                                attempts,
                                exit_code: raw.exit_code,
                                stdout: raw.stdout,
                                stderr: raw.stderr,
                                duration,
                            };
                            executions.push(execution);
                            if attempts >= MAX_ATTEMPTS {
                                return AttemptOutcome::Exhausted { executions };
                            }
                        }
                    }
                }
                Err(ExecutionFault::Cancelled) => {
                    let execution = HookExecution {
                        status: HookExecutionStatus::ExecutionFailed {
                            error: ExecutionFault::Cancelled.message().to_string(),
                        },
                        attempts,
                        exit_code: None,
                        stdout: String::new(),
                        stderr: String::new(),
                        duration,
                    };
                    executions.push(execution);
                    return AttemptOutcome::Cancelled { executions };
                }
                Err(fault) => {
                    let execution = HookExecution {
                        status: HookExecutionStatus::ExecutionFailed {
                            error: fault.message().to_string(),
                        },
                        attempts,
                        exit_code: None,
                        stdout: String::new(),
                        stderr: String::new(),
                        duration,
                    };
                    executions.push(execution);
                    if attempts >= MAX_ATTEMPTS {
                        return AttemptOutcome::Exhausted { executions };
                    }
                }
            }
        }
    }

    /// 派发一次 StopFailure 观察事件（best-effort，不递归）。
    ///
    /// 设计 §6 / 集成文档 §3：Stop subscription 重试耗尽后，Hook BC 先合成
    /// Block(StopHookExecutionFailed)，再尽力派发**恰好一次** StopFailure 通知。
    /// StopFailure subscription 自身的失败**NEVER** 递归触发新的 StopFailure，
    /// 也**NEVER** 改写已合成的 Stop Block 语义。
    ///
    /// StopFailure subscription 与普通 subscription 一样遵守
    /// `enabled` + `matcher` + `order` 稳定规则；其执行明细由调用方并入原 Stop
    /// HookOutcome.executions（本函数返回值的 `executions`）。
    async fn dispatch_stop_failure(
        &self,
        stop_invocation: &HookInvocation,
        error: String,
        cwd: &std::path::Path,
        env: &HashMap<String, String>,
        cancellation: &CancellationToken,
    ) -> HookOutcome {
        let turns = match stop_invocation {
            HookInvocation::Stop(input) => input.turns,
            _ => 0,
        };
        let invocation = HookInvocation::StopFailure(StopFailureInput { turns, error });

        // 复用主 dispatch 的 enabled + matcher + order 稳定规则（不再触发新的 StopFailure）。
        let mut matching: Vec<&HookSubscription> = self
            .subscriptions
            .iter()
            .filter(|s| {
                s.enabled
                    && s.point == HookPoint::StopFailure
                    && matcher_hits(&s.matcher, &invocation)
            })
            .collect();
        matching.sort_by_key(|s| s.order);

        let current_input = serde_json::to_value(&invocation).unwrap_or(serde_json::json!({}));
        let invocation_env = invocation_environment(&invocation, cwd, env);
        let mut all_executions: Vec<HookExecution> = Vec::new();
        for sub in matching {
            match self
                .execute_subscription(sub, &current_input, cwd, &invocation_env, cancellation)
                .await
            {
                AttemptOutcome::Success { executions, .. } => {
                    all_executions.extend(executions);
                }
                AttemptOutcome::Exhausted { executions } => {
                    all_executions.extend(executions);
                }
                AttemptOutcome::Cancelled { executions } => {
                    // StopFailure 取消：保留明细后终止，不改写已合成的 Stop Block 语义。
                    all_executions.extend(executions);
                    break;
                }
            }
        }

        // StopFailure 是观察点：其结果不改写已合成的 Stop Block。
        HookOutcome {
            executions: all_executions,
            directive: HookDirective::Continue,
            messages: Vec::new(),
        }
    }
}
