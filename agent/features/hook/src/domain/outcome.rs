//! Hook 执行结果与 Directive。
//!
//! 对应设计：`docs/design/02-modules/hook/README.md` §5。
//! 区分主动 Block 与 ExecutionFailed——主动 Block 不重试，ExecutionFailed 才重试。

use std::time::Duration;

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

/// Hook dispatch 的最终结果。
///
/// Runtime 拥有 directive 响应编排（如 Stop 阻断累计 15 次后第 16 次 RunFailed）。
#[derive(Debug, Clone)]
pub struct HookOutcome {
    /// 所有执行明细（含重试）。
    pub executions: Vec<HookExecution>,
    /// 最终 directive。
    pub directive: HookDirective,
}

impl HookOutcome {
    /// 创建一个 Proceed（Continue）结果，无执行明细。
    pub fn proceed() -> Self {
        Self {
            executions: Vec::new(),
            directive: HookDirective::Continue,
        }
    }
}
