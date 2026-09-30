//! 版本检查与自动更新 feature。
//!
//! 对应设计文档：`docs/snapshot/release-update-design.md`

mod constants;
pub(crate) use constants::LOG_TARGET;

mod release;
mod service;

pub use service::UpdateGateway;
