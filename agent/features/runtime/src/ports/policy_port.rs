//! Policy-owned Published Language 的 Runtime 兼容 re-export。
//! #918 完成消费切换后评估删除本兼容模块。

pub use policy::{
    ApprovalSubject, PolicyDecision, PolicyMode, PolicyPort, PolicyReason, PolicyRequest,
    PolicyRequestError,
};
