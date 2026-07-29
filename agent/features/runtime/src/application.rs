/// application/mod.rs — 用例编排层。
///
/// COLA 语义：消费 Port/Gateway，拥有用例决策，不依赖具体 Adapter。
/// 协议转换和运行时桥接已移入 `adapters/`。
pub(crate) mod client;
pub(crate) mod context;
pub(crate) mod hook;
pub(crate) mod interaction;
pub(crate) mod loop_engine;
pub(crate) mod model;
pub(crate) mod prompt;
pub(crate) mod reflection;
pub(crate) mod run;
pub(crate) mod session;
pub(crate) mod tool;
