/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:agent:policy";

mod adapters;
mod domain;

pub use adapters::AllowAllPolicy;
pub use domain::{
    ApprovalSubject, PolicyDecision, PolicyMode, PolicyPort, PolicyReason, PolicyRequest,
    PolicyRequestError,
};
