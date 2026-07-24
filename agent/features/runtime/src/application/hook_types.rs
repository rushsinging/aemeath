//! Runtime application hook adapter —— 纯值投影。
//!
//! 把 Hook BC 的 `hook::HookOutcome`（Hook BC 拥有的领域结果）
//! 投影成 Runtime 拥有的纯值 `RuntimeHookDispatch`。
//!
//! 设计约束（#925）：
//! - **纯转换**：不解析 stdout / JSON，不维护 Run 状态，不触碰 IO。
//!   Hook BC 已完成所有分类 / JSON 解析；此处仅做类型化搬运。
//! - **结构化 reason**：`hook::HookReason` 的全部 variant 都有对应的
//!   `RuntimeHookReason` variant，绝不压成仅 Debug 字符串。
//! - **execution 完整保留**：status / attempts / exit_code / stdout / stderr /
//!   duration 全部 1:1 搬运，重试轨迹（多次 execution）原样保留顺序与数量。
//! - **messages 顺序无损**：`hook::HookOutcome.messages`（BC 展示消息）按源顺序
//!   1:1 投影到 `RuntimeHookDispatch.messages`，point / source /
//!   execution_ordinal / attempt / kind / text 全部保留，不合并、不丢失来源。
//!
//! 对应设计：`docs/design/02-modules/runtime/`（Runtime 拥有 directive 响应编排，
//! 本投影仅产出 Runtime 可消费的值，编排逻辑由调用方负责）。

use std::time::Duration;

// ─── Runtime-owned projection types ───────────────────────────

/// Hook dispatch 在 Runtime 侧的纯值投影。
///
/// 由 [`project_hook_outcome`] 产出，是 Runtime 消费 hook 结果的稳定入口。
/// Runtime 拥有 directive 响应编排（如 Stop 阻断累计次数后合成 RunFailed），
/// 但那属于 Runtime 的领域逻辑，不在本投影范围内。
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeHookDispatch {
    /// 最终 directive（Continue / Block / Context / UpdatedInput / ContextAndInput）。
    pub directive: RuntimeHookDirective,
    /// 全部执行明细（含重试），顺序与源一致。
    pub executions: Vec<RuntimeHookExecution>,
    /// BC 保留的展示消息（按源顺序 1:1 投影，不合并、不丢失来源，#925）。
    pub messages: Vec<RuntimeHookDisplayMessage>,
    /// 最终 Block 的实际 subscription 与 execution；其它 directive 为 None。
    pub block_detail: Option<RuntimeHookBlockDetail>,
}

/// Runtime 视角下实际触发 Block 的 subscription 与 execution。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHookBlockDetail {
    pub command: String,
    pub execution_ordinal: u32,
    pub execution: RuntimeHookExecution,
}

/// Runtime 视角下的 hook directive。
///
/// 与 `hook::HookDirective` 一一对应，但去掉 `ContinueWith*` 前缀以突出
/// “继续 + 副作用”的 Runtime 语义。
#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeHookDirective {
    /// 允许继续。
    Continue,
    /// 主动阻断，携带结构化原因（非 Debug 字符串）。
    Block {
        /// 结构化阻断原因。
        reason: RuntimeHookReason,
    },
    /// 继续并向 LLM 对话流注入额外上下文。
    Context {
        /// 注入的上下文。
        context: String,
    },
    /// 继续并更新输入（调用方需重新执行 schema / Policy 校验）。
    UpdatedInput {
        /// hook BC 已解析的更新后输入。
        input: serde_json::Value,
    },
    /// 继续并同时注入上下文与更新输入。
    ContextAndInput {
        /// 注入的上下文。
        context: String,
        /// hook BC 已解析的更新后输入。
        input: serde_json::Value,
    },
}

/// 结构化 hook 阻断原因，对应 `hook::HookReason` 的全部 variant。
///
/// 必须保留 variant 边界：两个文本相同但 variant 不同的 reason
/// （例如 `JsonBlock{reason:"x"}` 与 `StopHookExecutionFailed{error:"x"}`）
/// 在投影后必须可区分。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeHookReason {
    /// Hook 脚本以非零退出码退出。
    ExitCode {
        /// 退出码。
        code: i32,
        /// stderr 摘要（无 stderr 时为空字符串）。
        stderr: String,
    },
    /// Hook 通过 JSON `decision:"block"` 主动声明阻断。
    JsonBlock {
        /// JSON 中的 reason 字段。
        reason: String,
    },
    /// Hook 通过 JSON `continue:false` 声明不允许停止（仅 Stop 语义）。
    JsonContinueFalse {
        /// JSON 中的 stopReason 字段。
        stop_reason: Option<String>,
    },
    /// Stop Hook 执行重试耗尽后合成的 Block。
    StopHookExecutionFailed {
        /// 最后一次执行失败的错误摘要。
        error: String,
    },
    /// `failure_policy=Block` 的普通 Hook 重试耗尽后合成的 Block。
    PolicyBlock {
        /// 最后一次执行失败的错误摘要。
        error: String,
    },
}

/// 单次 hook 命令执行的完整记录（Runtime 投影）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHookExecution {
    /// 执行状态。
    pub status: RuntimeHookExecutionStatus,
    /// 尝试次数（含第一次）。
    pub attempts: u8,
    /// 进程退出码（进程未正常退出时为 `None`）。
    pub exit_code: Option<i32>,
    /// stdout 原样输出（不解析）。
    pub stdout: String,
    /// stderr 原样输出。
    pub stderr: String,
    /// 总执行时长。
    pub duration: Duration,
}

/// Hook 执行状态（Runtime 投影）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeHookExecutionStatus {
    /// 执行成功（exit 0）。
    Success,
    /// Hook 主动阻断（非零 exit 或 JSON 声明 block）。
    Blocked,
    /// 执行失败（spawn/wait/IO/timeout/非法 JSON）。
    ExecutionFailed {
        /// 失败原因。
        error: String,
    },
}

// ─── Runtime-owned display-message types（#925 BC 展示消息投影）──

/// Hook 展示消息种类（Runtime 投影）。
///
/// 与 `hook::HookDisplayMessageKind` 一一对应：
/// - `AdditionalContext` ← JSON `additionalContext`（注入 LLM 对话流）；
/// - `SystemMessage` ← JSON `systemMessage`（显示在 TUI）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeHookDisplayMessageKind {
    /// 额外上下文（JSON `additionalContext`）。
    AdditionalContext,
    /// 系统消息（JSON `systemMessage`，警告等，显示在 TUI）。
    SystemMessage,
}

/// Hook BC 保留的展示消息（Runtime 投影）。
///
/// 由 [`project_message`] 产出，按源（`hook::HookDisplayMessage`）顺序 1:1 搬运，
/// 不合并、不丢失来源。六个字段全部投影：point / source / execution_ordinal /
/// attempt / kind / text。
///
/// `point` 直接复用 `hook::HookPoint`（Copy 域枚举，稳定共享词表，不重复定义
/// Runtime 镜像），其余字段为 Runtime 拥有的纯值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHookDisplayMessage {
    /// 触发点（直接复用 `hook::HookPoint`，Copy 域枚举）。
    pub point: hook::HookPoint,
    /// 来源（HookMatcher 稳定非秘密值：`All`="*"，`ToolName(name)`=name）。
    pub source: String,
    /// 执行序号（按 executions 聚合顺序，1-based）。
    pub execution_ordinal: u32,
    /// 该 subscription 内的成功 attempt 序号（含重试，1-based）。
    pub attempt: u8,
    /// 消息种类（Runtime 投影）。
    pub kind: RuntimeHookDisplayMessageKind,
    /// 消息文本。
    pub text: String,
}
pub fn project_hook_outcome(outcome: &hook::HookOutcome) -> RuntimeHookDispatch {
    RuntimeHookDispatch {
        directive: project_directive(&outcome.directive),
        executions: outcome.executions.iter().map(project_execution).collect(),
        messages: outcome.messages.iter().map(project_message).collect(),
        block_detail: outcome
            .block_detail
            .as_ref()
            .map(|detail| RuntimeHookBlockDetail {
                command: detail.command.clone(),
                execution_ordinal: detail.execution_ordinal,
                execution: project_execution(&detail.execution),
            }),
    }
}

fn project_directive(directive: &hook::HookDirective) -> RuntimeHookDirective {
    match directive {
        hook::HookDirective::Continue => RuntimeHookDirective::Continue,
        hook::HookDirective::Block { reason } => RuntimeHookDirective::Block {
            reason: project_reason(reason),
        },
        hook::HookDirective::ContinueWithContext { context } => RuntimeHookDirective::Context {
            context: context.clone(),
        },
        hook::HookDirective::ContinueWithUpdatedInput { input } => {
            RuntimeHookDirective::UpdatedInput {
                input: input.clone(),
            }
        }
        hook::HookDirective::ContinueWithContextAndInput { context, input } => {
            RuntimeHookDirective::ContextAndInput {
                context: context.clone(),
                input: input.clone(),
            }
        }
    }
}

fn project_reason(reason: &hook::HookReason) -> RuntimeHookReason {
    match reason {
        hook::HookReason::ExitCode { code, stderr } => RuntimeHookReason::ExitCode {
            code: *code,
            stderr: stderr.clone(),
        },
        hook::HookReason::JsonBlock { reason } => RuntimeHookReason::JsonBlock {
            reason: reason.clone(),
        },
        hook::HookReason::JsonContinueFalse { stop_reason } => {
            RuntimeHookReason::JsonContinueFalse {
                stop_reason: stop_reason.clone(),
            }
        }
        hook::HookReason::StopHookExecutionFailed { error } => {
            RuntimeHookReason::StopHookExecutionFailed {
                error: error.clone(),
            }
        }
        hook::HookReason::PolicyBlock { error } => RuntimeHookReason::PolicyBlock {
            error: error.clone(),
        },
    }
}

fn project_execution(execution: &hook::HookExecution) -> RuntimeHookExecution {
    RuntimeHookExecution {
        status: project_execution_status(&execution.status),
        attempts: execution.attempts,
        exit_code: execution.exit_code,
        stdout: execution.stdout.clone(),
        stderr: execution.stderr.clone(),
        duration: execution.duration,
    }
}

fn project_execution_status(status: &hook::HookExecutionStatus) -> RuntimeHookExecutionStatus {
    match status {
        hook::HookExecutionStatus::Success => RuntimeHookExecutionStatus::Success,
        hook::HookExecutionStatus::Blocked => RuntimeHookExecutionStatus::Blocked,
        hook::HookExecutionStatus::ExecutionFailed { error } => {
            RuntimeHookExecutionStatus::ExecutionFailed {
                error: error.clone(),
            }
        }
    }
}

fn project_message(message: &hook::HookDisplayMessage) -> RuntimeHookDisplayMessage {
    RuntimeHookDisplayMessage {
        point: message.point,
        source: message.source.clone(),
        execution_ordinal: message.execution_ordinal,
        attempt: message.attempt,
        kind: project_message_kind(&message.kind),
        text: message.text.clone(),
    }
}

fn project_message_kind(kind: &hook::HookDisplayMessageKind) -> RuntimeHookDisplayMessageKind {
    match kind {
        hook::HookDisplayMessageKind::AdditionalContext => {
            RuntimeHookDisplayMessageKind::AdditionalContext
        }
        hook::HookDisplayMessageKind::SystemMessage => RuntimeHookDisplayMessageKind::SystemMessage,
    }
}

impl From<&hook::HookOutcome> for RuntimeHookDispatch {
    fn from(outcome: &hook::HookOutcome) -> Self {
        project_hook_outcome(outcome)
    }
}
