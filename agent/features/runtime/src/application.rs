/// application/mod.rs — 用例编排层。
///
/// COLA 语义：消费 Port/Gateway，拥有用例决策，不依赖具体 Adapter。
/// 协议转换和运行时桥接已移入 `adapters/`。
pub(crate) mod active_run;
pub mod client;
pub mod context_coordination;
pub mod cost;
pub(crate) mod empty_hook;
pub mod hook_types;
pub mod interaction;
pub mod interaction_coordinator;
pub mod loop_engine;
pub mod main_loop;
pub mod model_invocation;
pub mod prompt;
pub mod reflection;
pub(crate) mod run_config;
pub mod run_execution_state;
#[cfg(test)]
mod run_execution_state_tests;
pub mod run_launcher;
pub mod run_preparer;
pub mod runtime_context;
pub mod runtime_context_factory;
#[cfg(test)]
mod runtime_context_factory_tests;
pub mod runtime_preparation;
#[cfg(test)]
mod runtime_preparation_tests;
pub mod scheduler;
pub mod service;
pub mod session_ingress;
#[cfg(test)]
#[path = "application/session_ingress_tests.rs"]
mod session_ingress_tests;
pub mod startup;
pub mod stop_hook_coordination;
pub mod subagent;
#[cfg(test)]
pub(crate) mod testing;
pub(crate) mod token_usage;
pub mod tool_coordination;
pub mod tool_result_materialization;
pub mod workspace_access;
