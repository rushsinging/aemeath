//! Hook dispatch adapter —— 按 point 匹配 subscription、串行执行、重试与聚合。
//!
//! 对应设计：`docs/design/02-modules/hook/README.md` §4 §6 §10 与
//! `01-run-loop-integration.md` §1 §2。
//!
//! - `Dispatcher` 实现 `HookDispatcher`：matcher 过滤 / order 排序 / Block 短路 /
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
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;

use share::config::domain::snapshot::HookExecutionPolicy;

use crate::domain::invocation::{HookInvocationData, HookPointData, StopFailureInput};
use crate::domain::outcome::{
    HookBlockDetail, HookDirectiveData, HookDisplayMessageData, HookDisplayMessageKindData,
    HookExecutionData, HookExecutionStatusData, HookOutcomeData,
};
use crate::domain::protocol::classify_output;
use crate::domain::subscription::{HookCommand, HookSubscription, SubscriptionError};

use crate::ports::{
    CancellationSignal, HookDispatchContextData, HookDispatcher,
    HookSubscriptionExecutionEventData, HookSubscriptionExecutionObserver,
    HookSubscriptionExecutionTerminalData,
};

pub(crate) use executor::{ExecutionFault, Executor, ProcessDriverExecutor};
#[cfg(test)]
use fake::{ScriptStep, Scripted};
use helpers::{
    classify_error_summary, last_error_of, matcher_hits, matcher_source, push_context,
    synthesize_cancelled_directive, synthesize_exhausted_directive,
};

/// Hook dispatcher：按 point 匹配 subscription、串行执行、重试与聚合。
///
/// 内部以 `Box<dyn Executor>` 持有执行端口，对外只暴露稳定构造入口
/// [`Dispatcher::try_new`]（cwd + env 白名单装配受管子进程执行器），
/// 不泄漏 `Executor` / `RawExecution` / `ExecutionFault` 等技术类型。
pub(crate) struct Dispatcher {
    subscriptions: Vec<HookSubscription>,
    executor: Box<dyn Executor>,
    execution_policy: HookExecutionPolicy,
    subscription_execution_observer: Option<Arc<dyn HookSubscriptionExecutionObserver>>,
}

/// Hook 子进程的 stdin payload：tagged enum 序列化后顶层双写扁平字段。
///
/// - `hook_event_name`：事件名（恒双写）；
/// - `session_id`：dispatch context 携带时双写（与 `AEMEATH_SESSION_ID` env 同源）。
///
/// 扁平解析器（cmux / Claude Code 生态工具按顶层字段提取会话标识）与
/// 既有按 tagged 结构解析的脚本同时可读；Runtime **NEVER** 参与拼装。
fn stdin_payload(invocation: &HookInvocationData, session_id: Option<&str>) -> serde_json::Value {
    let mut payload = serde_json::to_value(invocation).unwrap_or(serde_json::json!({}));
    let event_name = serde_json::to_value(invocation.point())
        .ok()
        .and_then(|value| value.as_str().map(str::to_string));
    let Some(object) = payload.as_object_mut() else {
        return payload;
    };
    if let Some(event_name) = event_name {
        object.insert(
            "hook_event_name".to_string(),
            serde_json::Value::String(event_name),
        );
    }
    if let Some(session_id) = session_id {
        object.insert(
            "session_id".to_string(),
            serde_json::Value::String(session_id.to_string()),
        );
    }
    payload
}

impl Dispatcher {
    /// 生产严格构造：Hook adapter 装配受管子进程执行器。
    ///
    /// cwd 是每次 dispatch 的 invocation context，不得在 Dispatcher 构造时冻结。
    /// `env_passthrough` 是额外透传给 hook 子进程的父环境变量 glob 模式
    /// （默认空 = 仅基础白名单；`AEMEATH_*` 按次变量恒为权威注入）。
    /// 任一 subscription 配置非法（如 Stop 配 failure_policy、非前置闸门配 Block）
    /// 即返回全部错误——与设计 §4「非法组合在 Config 校验阶段拒绝，而非运行时
    /// 静默忽略」一致。**NEVER** 静默丢弃非法 subscription。
    pub fn try_new(
        subscriptions: Vec<HookSubscription>,
        execution_policy: HookExecutionPolicy,
        env_passthrough: Vec<String>,
    ) -> Result<Self, Vec<SubscriptionError>> {
        Self::build(
            subscriptions,
            Box::new(ProcessDriverExecutor::new(
                crate::adapters::environment::managed_environment(&env_passthrough),
            )),
            execution_policy,
        )
    }

    /// 共用装配：严格校验全部 subscription 后装配执行器。
    fn build(
        subscriptions: Vec<HookSubscription>,
        executor: Box<dyn Executor>,
        execution_policy: HookExecutionPolicy,
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
            execution_policy,
            subscription_execution_observer: None,
        })
    }

    /// 测试专用：注入订阅执行观察者。
    #[cfg(test)]
    pub fn with_subscription_execution_observer(
        mut self,
        observer: Arc<dyn HookSubscriptionExecutionObserver>,
    ) -> Self {
        self.subscription_execution_observer = Some(observer);
        self
    }

    /// 测试专用：注入脚本化执行器，subscription 必须全部合法（否则 panic）。
    #[cfg(test)]
    fn with_scripted(subscriptions: Vec<HookSubscription>, executor: Scripted) -> Self {
        Self::with_scripted_and_attempt_limit(subscriptions, executor, 3)
    }

    #[cfg(test)]
    fn with_scripted_and_attempt_limit(
        subscriptions: Vec<HookSubscription>,
        executor: Scripted,
        max_attempts: u8,
    ) -> Self {
        Self::build(
            subscriptions,
            Box::new(executor),
            HookExecutionPolicy::new(max_attempts),
        )
        .expect("测试用 HookSubscription 必须全部合法")
    }
}

/// 单次 subscription 调度的内部结果。
enum AttemptOutcome {
    /// subscription 成功返回 directive（含业务 Block，业务 Block 不重试）。
    ///
    /// `executions` 携带该 subscription 的**全部** attempt 明细（含此前失败的
    /// 尝试与最终成功的尝试），确保 `HookOutcomeData.executions` 完整保留重试轨迹。
    /// `system_message` 为本次成功执行独立保留的 systemMessage（展示用）。
    Success {
        executions: Vec<HookExecutionData>,
        directive: HookDirectiveData,
        system_message: Option<String>,
    },
    /// 重试耗尽（ExecutionFailed 达到注入的 execution policy 上限）。
    Exhausted { executions: Vec<HookExecutionData> },
    /// 被 cancellation 终止（不重试，但仍保留这一次 attempt 的 ExecutionFailed 明细）。
    Cancelled { executions: Vec<HookExecutionData> },
}

#[async_trait]
impl HookDispatcher for Dispatcher {
    async fn dispatch(
        &self,
        invocation: HookInvocationData,
        cancellation: &dyn CancellationSignal,
    ) -> HookOutcomeData {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        self.dispatch_at(invocation, HookDispatchContextData::new(cwd), cancellation)
            .await
    }

    async fn dispatch_at(
        &self,
        invocation: HookInvocationData,
        context: HookDispatchContextData,
        cancellation: &dyn CancellationSignal,
    ) -> HookOutcomeData {
        let point = invocation.point();
        let subscription_execution_observer = context
            .subscription_execution_observer()
            .or(self.subscription_execution_observer.as_ref());

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
        let mut all_executions: Vec<HookExecutionData> = Vec::new();
        // BC 保留展示消息：按 executions 聚合顺序逐条保留 additionalContext / systemMessage。
        let mut messages: Vec<HookDisplayMessageData> = Vec::new();

        for sub in matching {
            let current_input = stdin_payload(&current_invocation, context.session_id());
            let invocation_env =
                invocation_environment(&current_invocation, context.cwd(), context.session_id());
            let outcome = self
                .execute_subscription(
                    sub,
                    &current_input,
                    context.cwd(),
                    &invocation_env,
                    subscription_execution_observer,
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
                        messages.push(HookDisplayMessageData {
                            point,
                            source: source.clone(),
                            execution_ordinal,
                            attempt,
                            kind: HookDisplayMessageKindData::SystemMessage,
                            text,
                        });
                    }
                    match directive {
                        HookDirectiveData::Continue => {}
                        HookDirectiveData::Block { reason } => {
                            let execution = all_executions
                                .last()
                                .expect("Block 成功执行必须保留最终 execution")
                                .clone();
                            return HookOutcomeData {
                                executions: all_executions,
                                directive: HookDirectiveData::Block { reason },
                                messages,
                                block_detail: Some(HookBlockDetail {
                                    command: sub.command.command.clone(),
                                    execution_ordinal,
                                    execution,
                                }),
                            };
                        }
                        HookDirectiveData::ContinueWithContext { context } => {
                            messages.push(HookDisplayMessageData {
                                point,
                                source: source.clone(),
                                execution_ordinal,
                                attempt,
                                kind: HookDisplayMessageKindData::AdditionalContext,
                                text: context.clone(),
                            });
                            push_context(&mut aggregated_context, context);
                        }
                        HookDirectiveData::ContinueWithUpdatedInput { input } => {
                            current_invocation.apply_updated_input(&input);
                            final_input = Some(input);
                        }
                        HookDirectiveData::ContinueWithContextAndInput { context, input } => {
                            messages.push(HookDisplayMessageData {
                                point,
                                source: source.clone(),
                                execution_ordinal,
                                attempt,
                                kind: HookDisplayMessageKindData::AdditionalContext,
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
                    if let HookDirectiveData::Block { .. } = directive {
                        let execution = all_executions
                            .last()
                            .expect("耗尽 Block 必须保留最终 execution")
                            .clone();
                        let block_detail = HookBlockDetail {
                            command: sub.command.command.clone(),
                            execution_ordinal: all_executions.len() as u32,
                            execution,
                        };
                        // Stop point 耗尽后尽力派发一次 StopFailure（不递归），
                        // 其执行明细并入原 Stop HookOutcomeData.executions。
                        if point == HookPointData::Stop {
                            let error = last_error_of(&all_executions).unwrap_or_default();
                            let sf_outcome = self
                                .dispatch_stop_failure(
                                    &current_invocation,
                                    error,
                                    context.cwd(),
                                    context.session_id(),
                                    cancellation,
                                )
                                .await;
                            all_executions.extend(sf_outcome.executions);
                        }
                        return HookOutcomeData {
                            executions: all_executions,
                            directive,
                            messages,
                            block_detail: Some(block_detail),
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
                    let execution_ordinal = all_executions.len() as u32;
                    let execution = all_executions
                        .last()
                        .expect("取消 Block 必须保留最终 execution")
                        .clone();
                    return HookOutcomeData {
                        executions: all_executions,
                        directive,
                        messages,
                        block_detail: Some(HookBlockDetail {
                            command: sub.command.command.clone(),
                            execution_ordinal,
                            execution,
                        }),
                    };
                }
            }
        }

        // 全部 subscription 完成 —— 组装聚合 directive。
        let directive = match (aggregated_context, final_input) {
            (Some(ctx), Some(inp)) => HookDirectiveData::ContinueWithContextAndInput {
                context: ctx,
                input: inp,
            },
            (Some(ctx), None) => HookDirectiveData::ContinueWithContext { context: ctx },
            (None, Some(inp)) => HookDirectiveData::ContinueWithUpdatedInput { input: inp },
            (None, None) => HookDirectiveData::Continue,
        };
        HookOutcomeData {
            executions: all_executions,
            directive,
            messages,
            block_detail: None,
        }
    }
}

fn hook_script_file_name(command: &str) -> String {
    let executable = first_shell_command_word(command);
    std::path::Path::new(&executable)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("hook")
        .to_string()
}

fn first_shell_command_word(command: &str) -> String {
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;

    for character in command.trim_start().chars() {
        if escaped {
            word.push(character);
            escaped = false;
            continue;
        }
        match (quote, character) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('\''), _) => word.push(character),
            (Some('"'), '\\') => escaped = true,
            (Some('"'), _) => word.push(character),
            (Some(_), _) => word.push(character),
            (None, '\'') | (None, '"') => quote = Some(character),
            (None, '\\') => escaped = true,
            (None, character) if character.is_whitespace() => break,
            (None, _) => word.push(character),
        }
    }
    if escaped {
        word.push('\\');
    }
    if word.is_empty() {
        "hook".to_string()
    } else {
        word
    }
}

fn invocation_environment(
    invocation: &HookInvocationData,
    cwd: &std::path::Path,
    session_id: Option<&str>,
) -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert(
        "AEMEATH_HOOK_EVENT".to_string(),
        serde_json::to_string(&invocation.point()).unwrap_or_default(),
    );
    env.insert("AEMEATH_PROJECT_DIR".to_string(), cwd.display().to_string());
    env.insert("CLAUDE_PROJECT_DIR".to_string(), cwd.display().to_string());
    // 当前 Main Session id：所有 point 统一注入，外部集成据此捕获会话。
    if let Some(session_id) = session_id {
        env.insert("AEMEATH_SESSION_ID".to_string(), session_id.to_string());
    }
    match invocation {
        HookInvocationData::PreToolUse(input) => {
            env.insert("AEMEATH_TOOL_NAME".to_string(), input.tool_name.clone());
            env.insert(
                "AEMEATH_TOOL_INPUT".to_string(),
                input.tool_input.to_string(),
            );
        }
        HookInvocationData::PostToolUse(input) => {
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
        HookInvocationData::PostToolUseFailure(input) => {
            env.insert("AEMEATH_TOOL_NAME".to_string(), input.tool_name.clone());
            env.insert(
                "AEMEATH_TOOL_INPUT".to_string(),
                input.tool_input.to_string(),
            );
            env.insert("AEMEATH_TOOL_OUTPUT".to_string(), input.error.clone());
            env.insert("AEMEATH_TOOL_IS_ERROR".to_string(), "true".to_string());
        }
        HookInvocationData::Stop(input) => {
            env.insert(
                "AEMEATH_STOP_RUN_STEPS".to_string(),
                input.run_steps.to_string(),
            );
        }
        HookInvocationData::PermissionRequest(input)
        | HookInvocationData::PermissionDenied(input) => {
            env.insert(
                "AEMEATH_PERMISSION_TOOL_NAME".to_string(),
                input.tool_name.clone(),
            );
            env.insert(
                "AEMEATH_PERMISSION_RULE".to_string(),
                input.permission_rule.clone(),
            );
        }
        HookInvocationData::InstructionsLoaded(input) => {
            env.insert(
                "AEMEATH_INSTRUCTIONS_FILE_PATH".to_string(),
                input.file_path.clone(),
            );
            env.insert(
                "AEMEATH_INSTRUCTIONS_TYPE".to_string(),
                input.instruction_type.clone(),
            );
        }
        HookInvocationData::Notification(input) => {
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
        subscription_execution_observer: Option<&Arc<dyn HookSubscriptionExecutionObserver>>,
        cancellation: &dyn CancellationSignal,
    ) -> AttemptOutcome {
        let mut attempts: u8 = 0;
        let mut executions: Vec<HookExecutionData> = Vec::new();
        // 命令原样透传：项目目录只经 `AEMEATH_PROJECT_DIR` / `CLAUDE_PROJECT_DIR`
        // 环境变量注入（`invocation_environment`），shell 内用 `${AEMEATH_PROJECT_DIR}`
        // 展开；`{AEMEATH_PROJECT_DIR}` 占位符写法已移除，不再替换。
        let command = HookCommand::new(sub.command.command.clone());
        let script = hook_script_file_name(&command.command);
        Self::observe_subscription_execution(
            subscription_execution_observer,
            HookSubscriptionExecutionEventData::Started {
                point: sub.point,
                script: script.clone(),
                attempt: 1,
            },
        );
        loop {
            attempts += 1;
            if attempts > 1 {
                Self::observe_subscription_execution(
                    subscription_execution_observer,
                    HookSubscriptionExecutionEventData::AttemptChanged {
                        point: sub.point,
                        script: script.clone(),
                        attempt: attempts,
                    },
                );
            }
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
                                HookDirectiveData::Block { .. } => HookExecutionStatusData::Blocked,
                                _ => HookExecutionStatusData::Success,
                            };
                            let terminal = match status {
                                HookExecutionStatusData::Success => {
                                    HookSubscriptionExecutionTerminalData::Succeeded
                                }
                                HookExecutionStatusData::Blocked
                                | HookExecutionStatusData::Cancelled
                                | HookExecutionStatusData::ExecutionFailed { .. } => {
                                    HookSubscriptionExecutionTerminalData::Failed
                                }
                            };
                            let execution = HookExecutionData {
                                status,
                                attempts,
                                exit_code: raw.exit_code,
                                stdout: raw.stdout,
                                stderr: raw.stderr,
                                stdout_file: raw.stdout_file,
                                stderr_file: raw.stderr_file,
                                duration,
                            };
                            // 成功也必须保留 prior executions（此前失败的 attempt 明细），
                            // 使 HookOutcomeData.executions 完整反映全部重试轨迹。
                            executions.push(execution);
                            Self::observe_subscription_execution(
                                subscription_execution_observer,
                                HookSubscriptionExecutionEventData::Finished {
                                    point: sub.point,
                                    script,
                                    terminal,
                                },
                            );
                            return AttemptOutcome::Success {
                                executions,
                                directive,
                                system_message,
                            };
                        }
                        Err(err) => {
                            let error = classify_error_summary(&err);
                            let execution = HookExecutionData {
                                status: HookExecutionStatusData::ExecutionFailed {
                                    error: error.clone(),
                                },
                                attempts,
                                exit_code: raw.exit_code,
                                stdout: raw.stdout,
                                stderr: raw.stderr,
                                stdout_file: raw.stdout_file,
                                stderr_file: raw.stderr_file,
                                duration,
                            };
                            executions.push(execution);
                            if attempts >= self.execution_policy.max_attempts() {
                                Self::observe_subscription_execution(
                                    subscription_execution_observer,
                                    HookSubscriptionExecutionEventData::Finished {
                                        point: sub.point,
                                        script,
                                        terminal: HookSubscriptionExecutionTerminalData::Failed,
                                    },
                                );
                                return AttemptOutcome::Exhausted { executions };
                            }
                        }
                    }
                }
                Err(ExecutionFault::Cancelled) => {
                    let execution = HookExecutionData {
                        status: HookExecutionStatusData::Cancelled,
                        attempts,
                        exit_code: None,
                        stdout: String::new(),
                        stderr: String::new(),
                        stdout_file: None,
                        stderr_file: None,
                        duration,
                    };
                    executions.push(execution);
                    Self::observe_subscription_execution(
                        subscription_execution_observer,
                        HookSubscriptionExecutionEventData::Finished {
                            point: sub.point,
                            script,
                            terminal: HookSubscriptionExecutionTerminalData::Cancelled,
                        },
                    );
                    return AttemptOutcome::Cancelled { executions };
                }
                #[cfg(any(not(unix), test))]
                Err(ExecutionFault::Unsupported) => {
                    let execution = HookExecutionData {
                        status: HookExecutionStatusData::ExecutionFailed {
                            error: ExecutionFault::Unsupported.message(),
                        },
                        attempts,
                        exit_code: None,
                        stdout: String::new(),
                        stderr: String::new(),
                        stdout_file: None,
                        stderr_file: None,
                        duration,
                    };
                    executions.push(execution);
                    Self::observe_subscription_execution(
                        subscription_execution_observer,
                        HookSubscriptionExecutionEventData::Finished {
                            point: sub.point,
                            script,
                            terminal: HookSubscriptionExecutionTerminalData::Failed,
                        },
                    );
                    return AttemptOutcome::Exhausted { executions };
                }
                Err(fault) => {
                    let execution = HookExecutionData {
                        status: HookExecutionStatusData::ExecutionFailed {
                            error: fault.message(),
                        },
                        attempts,
                        exit_code: None,
                        stdout: String::new(),
                        stderr: String::new(),
                        stdout_file: None,
                        stderr_file: None,
                        duration,
                    };
                    executions.push(execution);
                    // #1614：超时是确定性失败——同输入重跑只会再次超时，
                    // 重试仅把调用方等待放大 max_attempts 倍（实测 Stop hook
                    // 600s×3≈30 分钟）。Timeout 单次终判，不进入重试循环。
                    if matches!(fault, ExecutionFault::Timeout)
                        || attempts >= self.execution_policy.max_attempts()
                    {
                        Self::observe_subscription_execution(
                            subscription_execution_observer,
                            HookSubscriptionExecutionEventData::Finished {
                                point: sub.point,
                                script,
                                terminal: HookSubscriptionExecutionTerminalData::Failed,
                            },
                        );
                        return AttemptOutcome::Exhausted { executions };
                    }
                }
            }
        }
    }

    fn observe_subscription_execution(
        observer: Option<&Arc<dyn HookSubscriptionExecutionObserver>>,
        event: HookSubscriptionExecutionEventData,
    ) {
        if let Some(observer) = observer {
            observer.observe(event);
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
    /// HookOutcomeData.executions（本函数返回值的 `executions`）。
    async fn dispatch_stop_failure(
        &self,
        stop_invocation: &HookInvocationData,
        error: String,
        cwd: &std::path::Path,
        session_id: Option<&str>,
        cancellation: &dyn CancellationSignal,
    ) -> HookOutcomeData {
        let run_steps = match stop_invocation {
            HookInvocationData::Stop(input) => input.run_steps,
            _ => 0,
        };
        let invocation = HookInvocationData::StopFailure(StopFailureInput { run_steps, error });

        // 复用主 dispatch 的 enabled + matcher + order 稳定规则（不再触发新的 StopFailure）。
        let mut matching: Vec<&HookSubscription> = self
            .subscriptions
            .iter()
            .filter(|s| {
                s.enabled
                    && s.point == HookPointData::StopFailure
                    && matcher_hits(&s.matcher, &invocation)
            })
            .collect();
        matching.sort_by_key(|s| s.order);

        let current_input = stdin_payload(&invocation, session_id);
        let invocation_env = invocation_environment(&invocation, cwd, session_id);
        let mut all_executions: Vec<HookExecutionData> = Vec::new();
        for sub in matching {
            match self
                .execute_subscription(
                    sub,
                    &current_input,
                    cwd,
                    &invocation_env,
                    None,
                    cancellation,
                )
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
        HookOutcomeData {
            executions: all_executions,
            directive: HookDirectiveData::Continue,
            messages: Vec::new(),
            block_detail: None,
        }
    }
}
