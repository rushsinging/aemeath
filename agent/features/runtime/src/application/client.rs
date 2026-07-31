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

#[cfg(test)]
pub(crate) use accessors::SessionModelState;
pub(crate) use accessors::{RuntimeContextAssemblyError, SessionRuntime};
pub(crate) use mapping::{
    map_finalize_cause_to_sdk, message_to_sdk, skill_snapshot_to_sdk, workspace_context_to_sdk,
};

// 对外仅发布 Composition 装配所需的 workspace bootstrap。
pub use accessors::AgentClientImpl;
pub use bootstrap::{
    build_agent_runner, resolve_concurrency_limits, resolve_model_runtime_settings,
    AgentRunnerAssembly, ModelRuntimeSettings,
};
pub use from_args::{
    from_args_with_workspace, InitialProviderAssembly, PromptAssembly,
    RuntimeBootstrapDependencies, RuntimeCoreDependencies, RuntimeToolAssemblyDependencies,
    SessionBootstrapAssembly, SkillBootstrapAssembly,
};
pub use mapping::config_snapshot_to_sdk;
pub use resume_helper::{resume_session_to_backing, ResumeError};
