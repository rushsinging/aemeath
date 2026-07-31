//! Connect 向导的稳定错误分类。
//!
//! 与 [`docs/design/02-modules/config/02-provider-catalog-and-connect.md §8.1`](../../../../../../docs/design/02-modules/config/02-provider-catalog-and-connect.md)
//! 对齐。Adapter 错误必须在 Config ↔ Provider / Storage 边界映射，禁止
//! 泄漏临时路径、HTTP body 或供应商 wire DTO。

use crate::ports::ProviderProbeErrorKind;

use super::command::ConnectCommand;
use super::states::{ConnectRevision, ConnectStage};

/// Connect 命令的统一错误类型。
///
/// 每个变体都承载足够上下文用于 UI 投影；调用方**不得**用字符串匹配推导
/// 状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectError {
    /// 命令在当前 stage 不可执行。
    InvalidTransition {
        command: &'static str,
        actual: ConnectStage,
    },
    /// 命令携带的 revision 与 session 当前 revision 不一致；service 拒绝
    /// 处理任何状态变更。
    StaleRevision {
        actual: ConnectRevision,
        provided: ConnectRevision,
    },
    /// 单字段 / 跨字段校验失败。
    Validation { field: &'static str, reason: String },
    /// 引用了 Catalog 中不存在的固定 source / driver。
    CatalogUnavailable { reason: String },
    /// Provider 探测失败。`kind` 镜像 [`ProviderProbeErrorKind`]。
    ProbeFailed {
        kind: ProviderProbeErrorKind,
        message: String,
    },
    /// commit 端口返回的并发冲突；UI 应引导用户重载并重试。
    PersistConflict { expected: u64 },
    /// commit 端口返回的非冲突持久化错误。
    PersistFailed {
        kind: PersistErrorKind,
        message: String,
    },
    /// 当前 session 没有连接 commit 端口；保存路径尚未启用。
    PersistUnavailable,
    /// Config 全局配置在交互终端中不存在，且当前 stdin/stdout 不是 TTY。
    InteractiveSetupRequired,
    /// Bootstrap 回滚请求因 digest / identity 失配被拒绝。
    BootstrapRollbackRefused { reason: String },
}

/// 持久化错误分类。`ConnectError::PersistFailed` 的子类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistErrorKind {
    Serialization,
    Io,
    PermissionDenied,
    UnsupportedDurability,
    Internal,
}

impl ConnectError {
    /// 面向 UI 的简短中文描述。TUI 可以直接渲染；SDK 进一步包装。
    pub fn display_message(&self) -> String {
        match self {
            ConnectError::InvalidTransition { command, actual } => {
                format!("{command} 不能在 {actual:?} 阶段执行")
            }
            ConnectError::StaleRevision { actual, provided } => {
                format!(
                    "向导 revision 已变更（实际 {actual:?}，调用 {provided:?}）；请重载当前视图后再操作"
                )
            }
            ConnectError::Validation { field, reason } => {
                format!("{field} 校验失败：{reason}")
            }
            ConnectError::CatalogUnavailable { reason } => {
                format!("Provider Catalog 不可用：{reason}")
            }
            ConnectError::ProbeFailed { kind, .. } => {
                format!("连接探测失败：{kind:?}")
            }
            ConnectError::PersistConflict { expected } => {
                format!("全局配置已被其他进程修改（期望 revision {expected}）；请重载并重试")
            }
            ConnectError::PersistFailed { kind, message } => {
                format!("保存失败（{kind:?}）：{message}")
            }
            ConnectError::PersistUnavailable => "保存路径尚未启用".to_string(),
            ConnectError::InteractiveSetupRequired => {
                "请在交互终端运行 `aemeath connect` 完成初始化".to_string()
            }
            ConnectError::BootstrapRollbackRefused { reason } => {
                format!("默认配置回滚被拒绝：{reason}")
            }
        }
    }
}

/// 调用方便利函数：把 [`crate::connect::ConnectCommand`] 转为静态命令名。
///
/// 用于 [`ConnectError::InvalidTransition`] 等错误投影。
pub(crate) fn command_name(command: &ConnectCommand) -> &'static str {
    match command {
        ConnectCommand::SelectProvider { .. } => "SelectProvider",
        ConnectCommand::ConfirmOverwrite => "ConfirmOverwrite",
        ConnectCommand::RejectOverwrite => "RejectOverwrite",
        ConnectCommand::SetEndpoint { .. } => "SetEndpoint",
        ConnectCommand::SetCredential { .. } => "SetCredential",
        ConnectCommand::SetProviderUserAgent { .. } => "SetProviderUserAgent",
        ConnectCommand::SelectRecommendedModel { .. } => "SelectRecommendedModel",
        ConnectCommand::EnterCustomModel => "EnterCustomModel",
        ConnectCommand::SetCustomModel { .. } => "SetCustomModel",
        ConnectCommand::SetGlobalDefault { .. } => "SetGlobalDefault",
        ConnectCommand::SkipProbe => "SkipProbe",
        ConnectCommand::BeginProbe => "BeginProbe",
        ConnectCommand::ContinueAfterProbe => "ContinueAfterProbe",
        ConnectCommand::EditAfterProbeFailure => "EditAfterProbeFailure",
        ConnectCommand::ConfirmSave => "ConfirmSave",
    }
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod error_tests;
