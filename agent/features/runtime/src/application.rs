/// application/mod.rs — 用例编排层。
///
/// COLA 语义：消费 Port/Gateway，拥有用例决策，不依赖具体 Adapter。
/// 协议转换和运行时桥接已移入 `adapters/`。
pub mod client;
pub mod context;
pub mod hook;
pub mod interaction;
pub mod loop_engine;
pub mod model;
pub mod prompt;
pub mod reflection;
pub mod run;
pub mod session;
pub mod tool;
