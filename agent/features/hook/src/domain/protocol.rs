//! Hook 执行协议真值表。
//!
//! 对应设计：`docs/design/02-modules/hook/README.md` §5。
//! 将单次 hook 执行的原始结果（exit code + stdout JSON）分类为 directive，
//! 并依据能力矩阵校验非阻塞 point 的 Block。
//!
//! #924 typed 分类：`classify_directive` 返回 `Result<HookDirective, ClassifyError>`。
//! - exit 0 + 非法 JSON → `Err(InvalidJson)`；
//! - 能力矩阵违规 → `Err(Protocol{...})`；
//! - exit 1/2/127（任意非零）→ 阻塞 point `Ok(Block)`，非阻塞 point `Err(Protocol{BlockOnNonBlocking})`。

use crate::domain::invocation::HookPoint;
use crate::domain::outcome::{ClassifyError, HookDirective, HookReason, ProtocolViolation};

/// stdout/stderr 大小上限（字节）。超出部分截断。
pub(crate) const OUTPUT_MAX_BYTES: usize = 8192;

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

pub(crate) fn default_true() -> bool {
    true
}

/// 截断超长输出。
pub(crate) fn truncate(text: &str) -> String {
    if text.len() <= OUTPUT_MAX_BYTES {
        text.to_string()
    } else {
        text[..text.floor_char_boundary(OUTPUT_MAX_BYTES)].to_string()
    }
}

/// 将单次 hook 执行的原始结果分类为 directive。
///
/// # 真值表（#924 typed 分类）
///
/// | exit_code | stdout | → `Result` |
/// |---|---|---|
/// | 0 | 空 | `Ok(Continue)` |
/// | 0 | 合法 JSON | `Ok(directive)`（decision/context/updatedInput） |
/// | 0 | 非法 JSON | `Err(InvalidJson)` |
/// | 1 / 2 / 127 等任意非零 | 任意 | 阻塞 point `Ok(Block)`；非阻塞 point `Err(Protocol{BlockOnNonBlocking})` |
/// | None | 任意 | `Err(MissingExitCode)`（进程未正常退出，进入 ExecutionFailed 可重试） |
///
/// # 能力矩阵校验（设计 §3）
///
/// - `can_block=false` 收到 Block → `Err(Protocol{BlockOnNonBlocking})`；
/// - `can_modify_input=false` 收到 UpdatedInput → `Err(Protocol{UpdatedInputOnNonModifiable})`；
/// - `can_add_context=false` 收到 Context → `Err(Protocol{ContextOnNonContextual})`。
///
/// 分类失败（`Err`）对应 ExecutionFailed 路径，可重试；业务 Block 永不重试。
pub fn classify_directive(
    point: HookPoint,
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
) -> Result<HookDirective, ClassifyError> {
    // 公共签名保持兼容：丢弃 system_message，只返回 directive。
    classify_output(point, exit_code, stdout, stderr).map(|(directive, _system_message)| directive)
}

/// 将单次 hook 执行的原始结果分类为 directive，并**单独保留** system_message。
///
/// 与 [`classify_directive`] 的唯一区别：返回 `(directive, system_message)`，其中
/// `system_message` 取自 JSON `systemMessage` 字段，与 directive 无关、独立保留
/// （`additionalContext` 仍折叠进 directive，由 dispatcher 再展开为逐条
/// [`HookDisplayMessage`](crate::domain::outcome::HookDisplayMessage)）。
///
/// 分类失败（`Err`）时不携带 system_message（ExecutionFailed 路径不展示消息）。
pub(crate) fn classify_output(
    point: HookPoint,
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
) -> Result<(HookDirective, Option<String>), ClassifyError> {
    let meta = point.metadata();

    // ── exit_code=None：进程未正常退出，缺少退出码 → MissingExitCode ──
    // 必须优先于 stdout 解析：不得按空 stdout 误判为 Continue。
    let code = match exit_code {
        Some(c) => c,
        None => return Err(ClassifyError::MissingExitCode),
    };

    // ── 非零 exit → Block（能力校验后）；未解析 JSON，无 system_message ──
    if code != 0 {
        let block_reason = HookReason::ExitCode {
            code,
            stderr: truncate(stderr.trim()),
        };
        return enforce_block_permission(meta, block_reason).map(|d| (d, None));
    }

    // ── exit 0 + 空 stdout → Continue ──
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok((HookDirective::Continue, None));
    }

    // ── exit 0 + 尝试解析 JSON；非法 JSON → typed InvalidJson ──
    let json: HookJsonOutput = match serde_json::from_str(trimmed) {
        Ok(j) => j,
        Err(e) => {
            return Err(ClassifyError::InvalidJson {
                raw: truncate(trimmed),
                error: e.to_string(),
            });
        }
    };

    // ── system_message 独立保留（不折叠进 directive）──
    let system_message = json.system_message;

    // ── JSON decision:block → Block ──
    if json.decision.as_deref() == Some("block") {
        let reason = json.reason.unwrap_or_default();
        return enforce_block_permission(meta, HookReason::JsonBlock { reason })
            .map(|d| (d, system_message));
    }

    // ── JSON continue:false → Block（Stop 语义） ──
    if !json.r#continue {
        return enforce_block_permission(
            meta,
            HookReason::JsonContinueFalse {
                stop_reason: json.stop_reason,
            },
        )
        .map(|d| (d, system_message));
    }

    // ── 提取 additional_context 与 updated_input，并按能力矩阵校验 ──
    // 违规优先级：Block（上方已校验）> UpdatedInput > Context，
    // 与设计 §3 能力矩阵列举顺序一致。
    let context = json.additional_context;
    let updated_input = json
        .hook_specific_output
        .as_ref()
        .and_then(|h| h.get("updatedInput"))
        .cloned();

    if updated_input.is_some() && !meta.can_modify_input {
        return Err(ClassifyError::Protocol {
            violation: ProtocolViolation::UpdatedInputOnNonModifiable,
        });
    }
    if context.is_some() && !meta.can_add_context {
        return Err(ClassifyError::Protocol {
            violation: ProtocolViolation::ContextOnNonContextual,
        });
    }

    let directive = match (context, updated_input) {
        (Some(ctx), Some(inp)) => HookDirective::ContinueWithContextAndInput {
            context: ctx,
            input: inp,
        },
        (Some(ctx), None) => HookDirective::ContinueWithContext { context: ctx },
        (None, Some(inp)) => HookDirective::ContinueWithUpdatedInput { input: inp },
        (None, None) => HookDirective::Continue,
    };
    Ok((directive, system_message))
}

/// 能力矩阵校验：Block 仅允许出现在可阻断 point。
///
/// - `can_block=true` → `Ok(Block)`（业务 Block，永不重试）；
/// - `can_block=false` → `Err(Protocol{BlockOnNonBlocking})`（协议级故障，可重试）。
fn enforce_block_permission(
    meta: crate::domain::metadata::HookPointMetadata,
    reason: HookReason,
) -> Result<HookDirective, ClassifyError> {
    if meta.can_block {
        Ok(HookDirective::Block { reason })
    } else {
        Err(ClassifyError::Protocol {
            violation: ProtocolViolation::BlockOnNonBlocking,
        })
    }
}
