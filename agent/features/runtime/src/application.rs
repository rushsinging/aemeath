/// application/mod.rs — 应用层：用例编排（Agent Execution 核心域 + 支撑域协调）
pub(crate) mod active_run;
pub mod client;
pub mod context_coordination;
pub mod cost;
pub mod hook_adapter;
#[cfg(test)]
mod hook_adapter_tests;
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
pub mod suspension_mapping;
#[cfg(test)]
mod suspension_mapping_tests;
#[cfg(test)]
pub(crate) mod testing;
pub(crate) mod token_usage;
pub mod tool_coordination;
pub(crate) mod tool_execution_adapters;
pub mod tool_result_materialization;
