/// Context Management crate — 对话历史容器、上下文压缩、token 预算、提示组装、记忆注入。
///
/// 设计文档：`docs/design/02-modules/context-management/README.md`
pub const LOG_TARGET: &str = "aemeath:context";

pub mod budget;
pub mod compact;
pub mod context_port;
pub mod memory_inject;
pub mod prompt;
pub mod session;
