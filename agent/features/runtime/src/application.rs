/// application/mod.rs — 用例编排层。
///
/// COLA 语义：消费 Port/Gateway，拥有用例决策，不依赖具体 Adapter。
/// 协议转换和运行时桥接已移入 `adapters/`。
pub(crate) mod active_run;
pub mod client;
pub mod context_coordination;
pub mod cost;
pub mod interaction;
pub mod loop_engine;
pub mod main_loop;
pub mod model_invocation;
pub mod prompt;
pub mod reflection;
pub mod resources;
pub(crate) mod run_config;
pub mod run_launcher;
pub mod runtime_context;
pub mod scheduler;
pub mod service;
pub mod startup;
pub mod subagent;
#[cfg(test)]
pub(crate) mod testing;
pub(crate) mod token_usage;
pub mod tool_coordination;
pub mod tool_result_materialization;
