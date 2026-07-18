pub(crate) const LOG_TARGET: &str = "aemeath:agent:policy";
const _: &str = LOG_TARGET;
/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
mod adapters;
mod domain;

pub use adapters::AllowAllPolicy;
pub use domain::{
    ApprovalSubject, PolicyDecision, PolicyMode, PolicyPort, PolicyReason, PolicyRequest,
    PolicyRequestError,
};
