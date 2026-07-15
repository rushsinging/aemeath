//! Runtime 只消费 Context Management 发布的 OHS；本地不重复定义契约。

pub use context::api::context_port::*;

#[cfg(test)]
#[path = "context_port_tests.rs"]
mod tests;
