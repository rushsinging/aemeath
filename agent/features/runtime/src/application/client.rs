mod accessors;
mod bootstrap;
mod from_args;
mod mapping;
pub mod resume_helper;
pub(super) mod session_query;
mod trait_chat;
mod trait_impl;
mod trait_memory;
pub(crate) mod trait_model;
mod trait_reflection;
mod trait_session;

pub(crate) use accessors::*;
#[allow(unused_imports)]
pub(crate) use from_args::*;
#[allow(unused_imports)]
pub(crate) use mapping::*;

// 对外仅发布 Composition 装配所需的 workspace bootstrap。
pub use accessors::AgentClientImpl;
pub use bootstrap::{
    build_agent_runner, resolve_concurrency_limits, resolve_model_runtime_settings,
    AgentRunnerAssembly, ChatBootstrapArgs, ModelRuntimeSettings,
};
pub use from_args::{
    from_args_with_workspace, InitialProviderAssembly, PromptAssembly,
    RuntimeBootstrapDependencies, RuntimeCoreDependencies, RuntimeToolAssemblyDependencies,
    SessionBootstrapAssembly, SkillBootstrapAssembly,
};
pub use mapping::config_snapshot_to_sdk;
pub use resume_helper::{resume_session_to_backing, ResumeError};
