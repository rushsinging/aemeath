//! Dispatcher 内部辅助函数（匹配 / 上下文合并 / 错误摘要 / directive 合成）。
//!
//! 从 `dispatcher.rs` 拆出：这些纯函数不依赖 `&self`，按职责分组在此，
//! 保持 `dispatcher.rs` 只含编排逻辑（struct + dispatch 主循环 + 重试循环）。

use crate::domain::invocation::{HookInvocationData, HookPointData};
use crate::domain::outcome::{
    ClassifyError, HookDirectiveData, HookExecutionData, HookExecutionStatusData, HookReasonData,
    ProtocolViolation,
};
use crate::domain::subscription::{HookFailurePolicy, HookMatcherData};

use super::executor::ExecutionFault;

/// 上下文合并的分隔符（多个 ContinueWithContext 的 context 按执行顺序拼接）。
pub(super) const CONTEXT_SEPARATOR: &str = "\n";

/// matcher 是否命中 invocation。
///
/// - `All` 匹配任意 invocation；
/// - `ToolName(name)` 仅在带工具名的 point（PreToolUse / PostToolUse /
///   PostToolUseFailure / PermissionRequest / PermissionDenied）上且工具名相等时命中。
pub(super) fn matcher_hits(matcher: &HookMatcherData, invocation: &HookInvocationData) -> bool {
    match matcher {
        HookMatcherData::All => true,
        HookMatcherData::ToolName(expected) => tool_name_of(invocation)
            .map(|actual| actual == expected)
            .unwrap_or(false),
    }
}

/// HookMatcherData 的稳定非秘密来源值（用于展示消息的 `source` 字段）。
///
/// - `All` → `"*"`；
/// - `ToolName(name)` → `name`（工具名是用户配置的稳定非秘密值）。
pub(super) fn matcher_source(matcher: &HookMatcherData) -> String {
    match matcher {
        HookMatcherData::All => "*".to_string(),
        HookMatcherData::ToolName(name) => name.clone(),
    }
}

/// 提取 invocation 携带的工具名（仅工具相关 point 有）。
fn tool_name_of(invocation: &HookInvocationData) -> Option<&str> {
    match invocation {
        HookInvocationData::PreToolUse(i) => Some(&i.tool_name),
        HookInvocationData::PostToolUse(i) => Some(&i.tool_name),
        HookInvocationData::PostToolUseFailure(i) => Some(&i.tool_name),
        HookInvocationData::PermissionRequest(i) => Some(&i.tool_name),
        HookInvocationData::PermissionDenied(i) => Some(&i.tool_name),
        _ => None,
    }
}

/// 按执行顺序拼接 context（多次 ContinueWithContext 聚合）。
pub(super) fn push_context(aggregated: &mut Option<String>, context: String) {
    match aggregated {
        Some(existing) => {
            existing.push_str(CONTEXT_SEPARATOR);
            existing.push_str(&context);
        }
        None => *aggregated = Some(context),
    }
}

/// 从 ClassifyError 生成中文错误摘要字符串。
pub(super) fn classify_error_summary(err: &ClassifyError) -> String {
    match err {
        ClassifyError::InvalidJson { error, .. } => {
            format!("hook stdout 非法 JSON：{error}")
        }
        ClassifyError::MissingExitCode => "hook 进程未正常退出，缺少退出码".to_string(),
        ClassifyError::Protocol { violation } => {
            format!("hook 协议违规：{}", violation_message(*violation))
        }
    }
}

/// 能力矩阵违规的中文描述。
fn violation_message(violation: ProtocolViolation) -> &'static str {
    match violation {
        ProtocolViolation::BlockOnNonBlocking => "非阻塞 HookPointData 收到 Block",
        ProtocolViolation::UpdatedInputOnNonModifiable => {
            "不可修改输入的 HookPointData 收到 UpdatedInput"
        }
        ProtocolViolation::ContextOnNonContextual => {
            "不可追加上下文的 HookPointData 收到 AdditionalContext"
        }
    }
}

/// 取最后一次 ExecutionFailed 的 error 摘要（用于合成 Block reason）。
pub(super) fn last_error_of(executions: &[HookExecutionData]) -> Option<String> {
    executions.iter().rev().find_map(|e| match &e.status {
        HookExecutionStatusData::ExecutionFailed { error } => Some(error.clone()),
        _ => None,
    })
}

/// 合成重试耗尽后的最终 directive（纯函数，无需 async）。
pub(super) fn synthesize_exhausted_directive(
    point: HookPointData,
    failure_policy: Option<HookFailurePolicy>,
    all_executions: &[HookExecutionData],
) -> HookDirectiveData {
    let error = last_error_of(all_executions).unwrap_or_default();
    match point {
        // Stop 固定 Block(StopHookExecutionFailed) —— 设计 §6，用户不可覆盖。
        HookPointData::Stop => HookDirectiveData::Block {
            reason: HookReasonData::StopHookExecutionFailed { error },
        },
        // 普通 Hook：配置 Block → Block(PolicyBlock)；未配置 → 默认 Continue。
        _ => match failure_policy {
            Some(HookFailurePolicy::Block) => HookDirectiveData::Block {
                reason: HookReasonData::PolicyBlock { error },
            },
            _ => HookDirectiveData::Continue,
        },
    }
}

/// 合成 Cancelled 后的最终 directive（纯函数，无需 async）。
pub(super) fn synthesize_cancelled_directive(
    point: HookPointData,
    failure_policy: Option<HookFailurePolicy>,
) -> HookDirectiveData {
    let error = ExecutionFault::Cancelled.message().to_string();
    match point {
        // Stop 取消按 Stop 固定语义合成 Block(StopHookExecutionFailed)。
        HookPointData::Stop => HookDirectiveData::Block {
            reason: HookReasonData::StopHookExecutionFailed { error },
        },
        _ => match failure_policy {
            Some(HookFailurePolicy::Block) => HookDirectiveData::Block {
                reason: HookReasonData::PolicyBlock { error },
            },
            _ => HookDirectiveData::Continue,
        },
    }
}
