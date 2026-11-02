mod accessors;
mod bootstrap;
pub(crate) mod compact_model;
mod from_args;
mod mapping;
pub mod resume_helper;
mod session_metadata_field;
pub(crate) mod session_model_persist;
pub(crate) mod session_query;
pub(crate) mod session_reasoning_persist;
mod session_switch;
mod skill_catalog_refresh;
mod startup_resume;
mod trait_chat;
mod trait_impl;
pub(crate) mod trait_model;
mod trait_reflection;
mod trait_session;

pub(crate) use accessors::SessionModelState;
pub(crate) use accessors::{RuntimeContextAssemblyError, SessionInputHandle, SessionRuntime};
pub use compact_model::{CompactModelResolver, SessionModelSlotData};
pub(crate) use mapping::{
    display_history_index_to_sdk, map_finalize_cause_to_sdk, message_to_sdk, skill_snapshot_to_sdk,
    workspace_context_to_sdk,
};
pub use skill_catalog_refresh::SkillCatalogRefresh;
// Compact 模型解析复用 model switch 的 binding 构造路径，避免重复实现。
pub(crate) use trait_model::build_provider_binding_from_runtime_model;

// 对外仅发布 Composition 装配所需的 workspace bootstrap。
pub use accessors::AgentClientImpl;
pub use bootstrap::{
    resolve_concurrency_limits, resolve_model_runtime_settings, wire_agent_runner,
    AgentRunnerAssemblyData, ModelRuntimeSettingsData,
};
pub use from_args::{
    wire_agent_client_from_args, InitialProviderAssemblyData, PromptAssemblyData,
    RuntimeBootstrapDependenciesData, RuntimeCoreDependenciesData, RuntimeIngressAssembly,
    RuntimeToolAssemblyDependenciesData, SessionBootstrapAssemblyData, SkillBootstrapAssemblyData,
};
pub use mapping::config_snapshot_to_sdk;
pub use resume_helper::resume_session_to_backing;
#[cfg(any(test, feature = "dev"))]
pub use session_switch::wire_test_session_switch;
pub use session_switch::{
    wire_session_switch_coordinator, BoundMainRun, OwnedSessionSharedPermit,
    SessionSwitchCoordinator, SessionSwitchCoordinatorBuilder, SessionSwitchDependencies,
};
