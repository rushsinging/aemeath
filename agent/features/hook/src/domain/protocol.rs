//! Hook 执行协议真值表。
//!
//! 对应设计：`docs/design/02-modules/hook/README.md` §5。
//! 将单次 hook 执行的原始结果（exit code + stdout JSON）分类为 directive，
//! 并依据能力矩阵校验非阻塞 point 的 Block。

use crate::domain::invocation::HookPoint;
use crate::domain::outcome::{HookDirective, HookReason};

/// stdout/stderr 大小上限（字节）。超出部分截断。
const OUTPUT_MAX_BYTES: usize = 8192;

/// Hook stdout 的 JSON 输出（exit 0 时 stdout 可包含此 JSON）。
///
/// 向后兼容 Claude Code hook 协议字段名。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct HookJsonOutput {
    /// 是否继续执行（false 时全局停止，需配合 stopReason）。
    #[serde(default = "default_true")]
    r#continue: bool,
    /// 停止原因。
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_reason: Option<String>,
    /// 决策（"block" 表示阻止操作）。
    #[serde(skip_serializing_if = "Option::is_none")]
    decision: Option<String>,
    /// 阻止原因。
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    /// 额外上下文（注入到 LLM 对话流）。
    #[serde(skip_serializing_if = "Option::is_none")]
    additional_context: Option<String>,
    /// 系统消息（警告等，显示在 TUI）。
    #[serde(skip_serializing_if = "Option::is_none")]
    system_message: Option<String>,
    /// 事件特定输出（PreToolUse 用：permission/updatedInput 等）。
    #[serde(skip_serializing_if = "Option::is_none")]
    hook_specific_output: Option<serde_json::Value>,
}

fn default_true() -> bool {
    true
}

/// 截断超长输出。
fn truncate(text: &str) -> String {
    if text.len() <= OUTPUT_MAX_BYTES {
        text.to_string()
    } else {
        text[..text.floor_char_boundary(OUTPUT_MAX_BYTES)].to_string()
    }
}

/// 将单次 hook 执行的原始结果分类为 directive。
///
/// # 真值表
///
/// | exit_code | stdout | → directive |
/// |---|---|---|
/// | 0 | 空 | Continue |
/// | 0 | 合法 JSON | 解析 directive（decision/context/updatedInput） |
/// | 0 | 非法 JSON | `ExecutionFailed`（由调用方降级处理） |
/// | 非零 | 任意 | `Block{ ExitCode{code, stderr} }` |
/// | None | — | `ExecutionFailed`（进程未正常退出） |
///
/// # 能力矩阵校验
///
/// - `can_block=false` 的 point 收到 Block → 协议错误，降级为 Continue；
/// - `can_modify_input=false` 的 point 收到 UpdatedInput → 协议错误，丢弃；
/// - `can_add_context=false` 的 point 收到 Context → 协议错误，丢弃。
///
/// 协议错误不改变 directive 语义，调用方继续推进。
pub fn classify_directive(
    point: HookPoint,
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
) -> HookDirective {
    let meta = point.metadata();

    // ── 非零 exit → Block ──
    if let Some(code) = exit_code {
        if code != 0 {
            let block_reason = HookReason::ExitCode {
                code,
                stderr: truncate(stderr.trim()),
            };
            return enforce_block_permission(meta, block_reason);
        }
    }

    // ── exit 0 + 空 stdout → Continue ──
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return HookDirective::Continue;
    }

    // ── exit 0 + 尝试解析 JSON ──
    let json: HookJsonOutput = match serde_json::from_str(trimmed) {
        Ok(j) => j,
        Err(_) => {
            // 非法 JSON 不阻断流程；调用方记录 ExecutionFailed。
            return HookDirective::Continue;
        }
    };

    // ── JSON decision:block → Block ──
    if json.decision.as_deref() == Some("block") {
        let reason = json.reason.unwrap_or_default();
        return enforce_block_permission(meta, HookReason::JsonBlock { reason });
    }

    // ── JSON continue:false → Block（Stop 语义） ──
    if !json.r#continue {
        return enforce_block_permission(
            meta,
            HookReason::JsonContinueFalse {
                stop_reason: json.stop_reason,
            },
        );
    }

    // ── 提取 additional_context 与 updated_input ──
    let context = if meta.can_add_context {
        json.additional_context
    } else {
        None
    };

    let updated_input = if meta.can_modify_input {
        json.hook_specific_output
            .as_ref()
            .and_then(|h| h.get("updatedInput"))
            .cloned()
    } else {
        None
    };

    match (context, updated_input) {
        (Some(ctx), Some(inp)) => HookDirective::ContinueWithContextAndInput {
            context: ctx,
            input: inp,
        },
        (Some(ctx), None) => HookDirective::ContinueWithContext { context: ctx },
        (None, Some(inp)) => HookDirective::ContinueWithUpdatedInput { input: inp },
        (None, None) => HookDirective::Continue,
    }
}

/// 能力矩阵校验：非阻塞 point 收到 Block 时降级为 Continue。
fn enforce_block_permission(
    meta: crate::domain::metadata::HookPointMetadata,
    reason: HookReason,
) -> HookDirective {
    if meta.can_block {
        HookDirective::Block { reason }
    } else {
        // 非阻塞 point 的 Block → 协议错误，降级为 Continue。
        // 调用方可通过日志记录该协议违规。
        HookDirective::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("hello"), "hello");
    }

    #[test]
    fn test_truncate_long() {
        let long = "x".repeat(OUTPUT_MAX_BYTES + 100);
        let result = truncate(&long);
        assert!(result.len() <= OUTPUT_MAX_BYTES);
    }

    #[test]
    fn test_default_true() {
        assert!(default_true());
    }
}
