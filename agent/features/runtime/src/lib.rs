pub(crate) const LOG_TARGET: &str = "aemeath:agent:runtime";

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量.
pub mod adapters;
pub mod application;
pub mod domain;
pub mod ports;

pub use adapters::tool_result_blob::AtomicBlobToolResultStore;
pub use application::active_run::ActiveRunRegistry;
pub use application::tool_result_materialization::{
    ToolResultMaterializationPolicy, ToolResultMaterializer,
};

pub use application::client::{
    from_args_with_workspace, resume_session_to_backing, AgentClientImpl, InitialProviderAssembly,
    PromptAssembly, ResumeError, RuntimeBootstrapDependencies, RuntimeCoreDependencies,
    RuntimeToolAssemblyDependencies, SessionBootstrapAssembly, SkillBootstrapAssembly,
};
// #1248 Task 3: RuntimeContextFactory is the narrow crate-root construction
// entry.  RuntimeServices stays internal; callers construct via
// RuntimeContextFactory::new(…).
pub use application::prompt::build::{build_system_prompt_parts, PromptContext};
pub use application::prompt::prompt_build_ext::build_static_prompt;
pub use application::runtime_context::ParentRunContextSource;
pub use application::runtime_context_factory::RuntimeContextFactory;
pub use application::runtime_preparation::{
    ParentRunCapabilities, PreparedRun, RunPreparationError, RunPreparationRequest,
    SessionSnapshot, SessionState,
};
pub use application::startup::{
    build_agent_runner, resolve_concurrency_limits, resolve_model_runtime_settings,
    AgentRunnerAssembly, ModelRuntimeSettings,
};
pub use ports::{ProviderBinding, ProviderBuildSpec, ProviderFactory, ProviderPort};
pub use sdk::{
    AgentClient, ChangeSet, ChatEvent, ChatRequest, ChatStream, CostInfo, ProjectContext,
    TaskSummary,
};

#[cfg(test)]
mod boundary_tests {
    use std::path::Path;

    #[test]
    fn runtime_source_does_not_name_task_persistence_or_legacy_projection() {
        fn assert_tree(path: &Path) {
            for entry in std::fs::read_dir(path).expect("read Runtime source tree") {
                let path = entry.expect("read Runtime source entry").path();
                if path.is_dir() {
                    assert_tree(&path);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    let source = std::fs::read_to_string(&path).expect("read Runtime source file");
                    assert!(
                        !source.contains(&["Task", "Persist"].concat()),
                        "{} must not name the Task persistence capability",
                        path.display()
                    );
                    assert!(
                        !source.contains(&["legacy_task_snapshot", "_from_access"].concat()),
                        "{} must not restore the legacy manual projection",
                        path.display()
                    );
                }
            }
        }

        assert_tree(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").as_path());
    }
}
