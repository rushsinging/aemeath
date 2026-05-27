//! SDK composition root adapter.
//!
//! 该模块是 CLI 允许直接接触 runtime 的唯一装配入口之一：负责把 SDK
//! 启动 DTO 交给 runtime，并返回 SDK trait object。调用方不应继续展开
//! runtime 内部类型。

use std::sync::Arc;

pub(crate) async fn agent_client_from_args(
    args: sdk::ChatBootstrapArgs,
) -> Result<Arc<dyn sdk::AgentClient>, sdk::SdkError> {
    Ok(Arc::new(::runtime::api::client::from_args(args).await?))
}

pub(crate) fn set_current_turn(turn: usize) {
    ::runtime::api::bootstrap::set_current_turn(turn);
}
