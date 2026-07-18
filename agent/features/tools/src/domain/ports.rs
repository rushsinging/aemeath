//! Tool 双端口（DDD §6.4.3）。
//!
//! 设计来源：`docs/design/02-modules/tools/02-ports-and-lifecycle.md`。
//!
//! # ToolCatalogPort
//!
//! 只读投影端口：根据 Registry Scope 与 Tool Profile 生成可见 Tool 快照。
//! 返回 `ToolDescriptor`，**禁止**返回 `Arc<dyn Tool>`、Registry handle 或
//! MCP client / transport。
//!
//! # ToolExecutionPort
//!
//! 执行端口：接收 `ToolInvocation`，在调用瞬间重新验证（Tool 存在、Profile 允许、
//! resources 可用、input 符合 schema），然后调用实际函数并标准化为 `ToolOutcome`。
//!
//! 故意保持单一 `ToolOutcome` 通道（含错误），避免调用方在 `Result::Err` 与
//! `ToolOutcome::Failure` 之间产生两套失败语义。

use async_trait::async_trait;

use super::{
    published_language::{
        RegistryScopeName, ToolCatalogError, ToolCatalogSnapshot, ToolInvocation,
        ToolOutcome as ToolExecutionOutcome, ToolProfileName,
    },
    CancellationSignal,
};

/// Tool Catalog 只读投影端口。
///
/// 消费方（Runtime）通过此端口获取可见工具快照，不接触 Registry 内部。
pub trait ToolCatalogPort: Send + Sync {
    /// 根据 Scope 和 Profile 生成当前可见 Tool 的只读快照。
    ///
    /// 保证：
    /// - ToolName 唯一、required resources 齐备、capabilities 被允许；
    /// - 组合 built-in 与 MCP Tool，但隐藏来源实现；
    /// - 返回的 `ToolDescriptor` 不包含 Tool 实例或函数指针。
    fn snapshot(
        &self,
        scope: &RegistryScopeName,
        profile: &ToolProfileName,
    ) -> Result<ToolCatalogSnapshot, ToolCatalogError>;
}

pub trait ToolExecutionContextBindingPort: Send + Sync {
    fn bind(&self, context: super::ToolExecutionContext) -> Result<(), String>;
    fn unbind(&self, run_id: &str);
}

/// RAII binding: every successful bind is paired with unbind on completion,
/// cancellation, panic, or future drop.
pub struct ToolExecutionContextBindingGuard {
    port: std::sync::Arc<dyn ToolExecutionContextBindingPort>,
    run_id: String,
}

impl ToolExecutionContextBindingGuard {
    pub fn bind(
        port: std::sync::Arc<dyn ToolExecutionContextBindingPort>,
        context: super::ToolExecutionContext,
    ) -> Result<Self, String> {
        let run_id = context.scope().run_id().to_string();
        port.bind(context)?;
        Ok(Self { port, run_id })
    }
}

impl Drop for ToolExecutionContextBindingGuard {
    fn drop(&mut self) {
        self.port.unbind(&self.run_id);
    }
}

/// Tool 执行端口。
///
/// Runtime 通过此端口执行单个 Tool 调用。执行前重新验证 Tool 存在性、
/// Profile 权限、resources 可用性和 input schema 合法性。
///
/// Policy、Hook、人工审批、timeout 和跨 Tool 并发不得下沉进此端口。
#[async_trait]
pub trait ToolExecutionPort: Send + Sync {
    /// 执行一次 Tool 调用。
    ///
    /// 返回 `ToolOutcome`（单一通道，含错误）。Tool 不存在返回
    /// `Failure(ToolUnavailable)`；schema 失败返回 `Failure(InvalidInput)`。
    async fn execute(
        &self,
        invocation: ToolInvocation,
        cancellation: &dyn CancellationSignal,
    ) -> ToolExecutionOutcome;
}
