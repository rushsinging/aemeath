//! Hook 执行结果与 Directive。
//!
//! 对应设计：`docs/design/02-modules/hook/README.md` §5。
//! 区分主动 Block 与 ExecutionFailed——主动 Block 不重试，ExecutionFailed 才重试。

use std::time::Duration;

use crate::domain::invocation::HookPoint;

// ─── HookDirective ────────────────────────────────────────────

/// Hook directive——调用方解释并推进自己的聚合。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookDirective {
    /// 允许继续。
    Continue,
    /// 主动阻断（exit 0 + JSON `decision:"block"` / `continue:false`，或任意非零 exit）。
    Block {
        /// 阻断原因。
        reason: HookReason,
    },
    /// 继续并注入额外上下文。
    ContinueWithContext {
        /// 注入到 LLM 对话流的额外上下文。
        context: String,
    },
    /// 继续并更新输入（调用方必须重新执行 schema/Policy 校验）。
    ContinueWithUpdatedInput {
        /// 更新后的输入。
        input: serde_json::Value,
    },
    /// 继续并注入上下文 + 更新输入。
    ContinueWithContextAndInput {
        /// 注入到 LLM 对话流的额外上下文。
        context: String,
        /// 更新后的输入。
        input: serde_json::Value,
    },
}

/// Hook 阻断原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookReason {
    /// Hook 脚本以非零退出码退出。
    /// 携带 exit code 与 stderr 摘要。
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
    /// 配置 `failure_policy=Block` 的普通 Hook 重试耗尽后合成的 Block。
    ///
    /// 仅适用于可配置 failure_policy 的前置闸门（Stop 走 `StopHookExecutionFailed`）。
    PolicyBlock {
        /// 最后一次执行失败的错误摘要。
        error: String,
    },
}

// ─── ClassifyError（#924 协议分类 TDD）─────────────────────────

/// `classify_directive` 的 typed 分类失败。
///
/// 对应设计 §5 真值表中的 ExecutionFailed 路径，与业务 Block（`HookReason`）严格区分：
/// - 业务 Block 是 Hook 的合法业务结果，永不重试；
/// - 分类失败是协议级故障，需进入 ExecutionFailed / 重试处理。
///
/// 分类规则：
/// - exit 0 + 非法 JSON → `InvalidJson`；
/// - `exit_code=None`（进程未正常退出）→ `MissingExitCode`；
/// - 能力矩阵违规（point 元数据不支持该 directive）→ `Protocol`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassifyError {
    /// exit 0 + 非法 JSON stdout。
    ///
    /// 不阻断流程，但调用方必须记录为 ExecutionFailed（可重试）。
    InvalidJson {
        /// 触发解析失败的原始 stdout（已按 `OUTPUT_MAX_BYTES` 截断）。
        raw: String,
        /// 解析错误摘要。
        error: String,
    },
    /// 进程未正常退出，缺少退出码（`exit_code=None`）。
    ///
    /// 进程未正常退出时没有退出码可供分类，必须进入 ExecutionFailed 可重试路径，
    /// **不得**按空 stdout 误判为 Continue。
    MissingExitCode,
    /// 能力矩阵违规：HookPoint 元数据不支持收到的 directive。
    ///
    /// 设计 §3：`can_block=false` 收到 Block、`can_modify_input=false` 收到
    /// UpdatedInput、`can_add_context=false` 收到 Context，均为协议错误。
    Protocol {
        /// 违规详情。
        violation: ProtocolViolation,
    },
}

/// 能力矩阵违规类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolViolation {
    /// 非阻塞 point（`can_block=false`）收到 Block
    /// （非零 exit / JSON `decision:"block"` / `continue:false`）。
    BlockOnNonBlocking,
    /// `can_modify_input=false` 的 point 收到 UpdatedInput。
    UpdatedInputOnNonModifiable,
    /// `can_add_context=false` 的 point 收到 AdditionalContext。
    ContextOnNonContextual,
}

// ─── HookExecution ────────────────────────────────────────────

/// 单次 Hook 命令执行的完整记录。
#[derive(Debug, Clone)]
pub struct HookExecution {
    /// 执行状态。
    pub status: HookExecutionStatus,
    /// 尝试次数（含第一次）。
    pub attempts: u8,
    /// 进程退出码（进程未正常退出时为 None）。
    pub exit_code: Option<i32>,
    /// stdout 输出。
    pub stdout: String,
    /// stderr 输出。
    pub stderr: String,
    /// 总执行时长。
    pub duration: Duration,
}

/// Hook 执行状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookExecutionStatus {
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

// ─── HookOutcome ──────────────────────────────────────────────

/// 触发最终阻断的 subscription 与实际执行记录。
#[derive(Debug, Clone)]
pub struct HookBlockDetail {
    /// 配置的实际执行命令（变量已按当前 invocation 展开）。
    pub command: String,
    /// 在 HookOutcome 全量执行记录中的 1-based 序号。
    pub execution_ordinal: u32,
    /// 触发阻断的最终执行记录。
    pub execution: HookExecution,
}

/// Hook dispatch 的最终结果。
///
/// Runtime 拥有 directive 响应编排（如 Stop 阻断累计 15 次后第 16 次 RunFailed）。
#[derive(Debug, Clone)]
pub struct HookOutcome {
    /// 所有执行明细（含重试）。
    pub executions: Vec<HookExecution>,
    /// 最终 directive。
    pub directive: HookDirective,
    /// BC 保留的展示消息（按 executions 聚合顺序逐条保留，不合并、不丢失来源）。
    ///
    /// 与 `directive` 的聚合 context 不同：`messages` 按「每条 subscription 的每次成功
    /// 执行」逐条保留 additionalContext / systemMessage，供调用方（Runtime / TUI）原样展示。
    pub messages: Vec<HookDisplayMessage>,
    /// 最终 directive 为 Block 时，标识实际阻断 subscription；其它 directive 为 None。
    pub block_detail: Option<HookBlockDetail>,
}

impl HookOutcome {
    /// 创建一个 Proceed（Continue）结果，无执行明细、无展示消息。
    pub fn proceed() -> Self {
        Self {
            executions: Vec::new(),
            directive: HookDirective::Continue,
            messages: Vec::new(),
            block_detail: None,
        }
    }
}

// ─── HookDisplayMessage（BC 保留展示消息，#925）─────────────────

/// Hook 展示消息种类。
///
/// 对应 Claude Code hook 协议的两个独立展示字段：
/// - `AdditionalContext` ← JSON `additionalContext`（注入 LLM 对话流）；
/// - `SystemMessage` ← JSON `systemMessage`（显示在 TUI）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookDisplayMessageKind {
    /// 额外上下文（JSON `additionalContext`）。
    AdditionalContext,
    /// 系统消息（JSON `systemMessage`，警告等，显示在 TUI）。
    SystemMessage,
}

/// Hook BC 保留的展示消息。
///
/// 按「每条 subscription 的每次成功执行」逐条保留，不合并、不丢失来源，
/// 供调用方（Runtime / TUI）原样展示。`source` 取 HookMatcher 的稳定非秘密值
/// （`All`="*"，`ToolName(name)`=name），`execution_ordinal` 按 executions 聚合顺序递增。
#[derive(Debug, Clone)]
pub struct HookDisplayMessage {
    /// 触发点。
    pub point: HookPoint,
    /// 来源（HookMatcher 稳定非秘密值：`All`="*"，`ToolName(name)`=name）。
    pub source: String,
    /// 执行序号（按 executions 聚合顺序，1-based）。
    pub execution_ordinal: u32,
    /// 该 subscription 内的成功 attempt 序号（含重试，1-based）。
    pub attempt: u8,
    /// 消息种类。
    pub kind: HookDisplayMessageKind,
    /// 消息文本。
    pub text: String,
}
