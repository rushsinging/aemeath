//! HookPoint 元数据与能力矩阵。
//!
//! 对应设计：`docs/design/02-modules/hook/README.md` §3。
//! 元数据由系统拥有，用户配置不直接声明 class。

use crate::domain::invocation::HookPoint;

/// Hook 功能分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookClass {
    /// 边界闸门（Session / SubRun / Prompt / Compact 等生命周期边界）。
    Boundary,
    /// 工具相关。
    Tool,
    /// 通知 / 观察。
    Notification,
}

/// HookPoint 能力声明。
///
/// 编码设计文档 §3 的能力矩阵。Hook adapter 依据此矩阵校验 HookDirective：
/// - `can_block=false` 收到 Block → 协议错误，进入 ExecutionFailed；
/// - `can_modify_input=false` 收到 UpdatedInput → 协议错误；
/// - `can_add_context=false` 收到 Context → 协议错误；
/// - Stop 收到任何 ContinueWith* → 协议错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookPointMetadata {
    /// 功能分类。
    pub class: HookClass,
    /// 是否允许主动阻断。
    pub can_block: bool,
    /// 是否允许修改输入。
    pub can_modify_input: bool,
    /// 是否允许追加上下文。
    pub can_add_context: bool,
    /// 是否允许用户配置 failure_policy=Block。
    pub failure_policy_configurable: bool,
}

impl HookPoint {
    /// 返回该触发点的能力矩阵。
    pub const fn metadata(self) -> HookPointMetadata {
        match self {
            // ── 前置闸门：可 Block、可改 input、可加 context ──
            Self::PreToolUse => HookPointMetadata {
                class: HookClass::Tool,
                can_block: true,
                can_modify_input: true,
                can_add_context: true,
                failure_policy_configurable: true,
            },
            Self::UserPromptSubmit => HookPointMetadata {
                class: HookClass::Boundary,
                can_block: true,
                can_modify_input: true,
                can_add_context: true,
                failure_policy_configurable: true,
            },
            Self::PreCompact => HookPointMetadata {
                class: HookClass::Boundary,
                can_block: true,
                can_modify_input: false,
                can_add_context: true,
                failure_policy_configurable: true,
            },
            Self::PermissionRequest => HookPointMetadata {
                class: HookClass::Boundary,
                can_block: true,
                can_modify_input: true,
                can_add_context: true,
                failure_policy_configurable: true,
            },
            Self::Elicitation => HookPointMetadata {
                class: HookClass::Boundary,
                can_block: true,
                can_modify_input: true,
                can_add_context: true,
                failure_policy_configurable: true,
            },
            Self::UserPromptExpansion => HookPointMetadata {
                class: HookClass::Boundary,
                can_block: true,
                can_modify_input: true,
                can_add_context: true,
                failure_policy_configurable: true,
            },
            // ── Stop 闸门：可 Block，不可改 input，不可加 context ──
            // failure_policy_configurable=false：Stop 执行失败固定为 Block，用户不可改。
            Self::Stop => HookPointMetadata {
                class: HookClass::Boundary,
                can_block: true,
                can_modify_input: false,
                can_add_context: false,
                failure_policy_configurable: false,
            },
            // ── 后置增强：不可 Block，不可改 input，可加 context ──
            Self::PostToolUse => HookPointMetadata {
                class: HookClass::Tool,
                can_block: false,
                can_modify_input: false,
                can_add_context: true,
                failure_policy_configurable: false,
            },
            Self::PostToolUseFailure => HookPointMetadata {
                class: HookClass::Tool,
                can_block: false,
                can_modify_input: false,
                can_add_context: true,
                failure_policy_configurable: false,
            },
            Self::PostCompact => HookPointMetadata {
                class: HookClass::Notification,
                can_block: false,
                can_modify_input: false,
                can_add_context: true,
                failure_policy_configurable: false,
            },
            Self::PostToolBatch => HookPointMetadata {
                class: HookClass::Tool,
                can_block: false,
                can_modify_input: false,
                can_add_context: true,
                failure_policy_configurable: false,
            },
            Self::ElicitationResult => HookPointMetadata {
                class: HookClass::Notification,
                can_block: false,
                can_modify_input: false,
                can_add_context: true,
                failure_policy_configurable: false,
            },
            // ── 生命周期：不可 Block，不可改 input，可加 context ──
            Self::SessionStart => HookPointMetadata {
                class: HookClass::Boundary,
                can_block: false,
                can_modify_input: false,
                can_add_context: true,
                failure_policy_configurable: false,
            },
            Self::SessionEnd => HookPointMetadata {
                class: HookClass::Boundary,
                can_block: false,
                can_modify_input: false,
                can_add_context: true,
                failure_policy_configurable: false,
            },
            Self::SubRunStart => HookPointMetadata {
                class: HookClass::Boundary,
                can_block: false,
                can_modify_input: false,
                can_add_context: true,
                failure_policy_configurable: false,
            },
            Self::SubRunStop => HookPointMetadata {
                class: HookClass::Boundary,
                can_block: false,
                can_modify_input: false,
                can_add_context: true,
                failure_policy_configurable: false,
            },
            Self::TaskCreated => HookPointMetadata {
                class: HookClass::Notification,
                can_block: false,
                can_modify_input: false,
                can_add_context: true,
                failure_policy_configurable: false,
            },
            Self::TaskCompleted => HookPointMetadata {
                class: HookClass::Notification,
                can_block: false,
                can_modify_input: false,
                can_add_context: true,
                failure_policy_configurable: false,
            },
            Self::Notification => HookPointMetadata {
                class: HookClass::Notification,
                can_block: false,
                can_modify_input: false,
                can_add_context: true,
                failure_policy_configurable: false,
            },
            Self::InstructionsLoaded => HookPointMetadata {
                class: HookClass::Notification,
                can_block: false,
                can_modify_input: false,
                can_add_context: true,
                failure_policy_configurable: false,
            },
            // ── 观察：全部 false ──
            Self::StopFailure => HookPointMetadata {
                class: HookClass::Notification,
                can_block: false,
                can_modify_input: false,
                can_add_context: false,
                failure_policy_configurable: false,
            },
            Self::PermissionDenied => HookPointMetadata {
                class: HookClass::Notification,
                can_block: false,
                can_modify_input: false,
                can_add_context: false,
                failure_policy_configurable: false,
            },
            Self::ConfigChange => HookPointMetadata {
                class: HookClass::Notification,
                can_block: false,
                can_modify_input: false,
                can_add_context: false,
                failure_policy_configurable: false,
            },
            Self::CwdChanged => HookPointMetadata {
                class: HookClass::Notification,
                can_block: false,
                can_modify_input: false,
                can_add_context: false,
                failure_policy_configurable: false,
            },
            Self::FileChanged => HookPointMetadata {
                class: HookClass::Notification,
                can_block: false,
                can_modify_input: false,
                can_add_context: false,
                failure_policy_configurable: false,
            },
            Self::TeammateIdle => HookPointMetadata {
                class: HookClass::Notification,
                can_block: false,
                can_modify_input: false,
                can_add_context: false,
                failure_policy_configurable: false,
            },
        }
    }
}
