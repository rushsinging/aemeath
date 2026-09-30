//! 版本检查与自动更新 feature。
//!
//! 对应设计文档：`docs/snapshot/release-update-design.md`

pub(crate) const LOG_TARGET: &str = "aemeath:agent:update";

mod release;
mod service;

pub use service::UpdateGateway;
