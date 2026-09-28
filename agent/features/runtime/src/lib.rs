//! Runtime：agent 会话循环、工具编排与装配入口。
//!
//! # Published Language（四类语法，#1713 收敛）
//!
//! | 组 | 实体 |
//! |---|---|
//! | 工厂 | `wire_agent_runner`、`wire_agent_client_from_args`（原 build_agent_runner/from_args_with_workspace 更名）、`wire_active_run_registry` |
//! | 角色和职能 | `ProviderPort`/`ProviderFactory`（runtime 自有 trait，provider crate 实现载荷转发）、`UsageSink`/`UnavailableUsageSink`（audit 降级路径）、`RuntimeContextFactory`（#1248 窄构造入口）、`ToolResultMaterializer`、`ActiveRunRegistry`、`AtomicBlobToolResultStore`、`AgentClientImpl`、`CompactModelResolver`、`ProviderCompactGenerator`、`ReflectionTaskAdapter`、`ParentRunContextSource` |
//! | 数据和生命周期 | 14 个 Data：ProviderBindingData/ProviderBuildSpecData（provider 装配）、ModelRuntimeSettingsData/PromptContextData、装配族 8（RuntimeBootstrapDependenciesData 已组合化：core+tool_assembly+agent_runner 嵌套，消除三层平铺）、ToolResultMaterializationPolicyData、reflection 数据 5 |
//! | Error | 出口统一 `sdk::SdkError`（架构既定）；10 个内部错误全 crate 内——**不折叠不 Data 化** |
//!
//! 删除与内部化：sdk 转发 7（纯冗余，消费方直连 sdk）；实测修正——
//! `ToolResultBlobPort`（签名载荷）、reflection 数据 5 + resume/Assembly 3
//! （集成测试消费）恢复 pub；契约测试 3 个搬 crate 内联。
//! 后续下架：`RuntimeLifecycleEvent`/`map_lifecycle_event` 根 re-export
//! （零 crate 外消费，内部走真实模块路径；定义处 pub 由架构测试钉住）。
//! 按 docs/design/03-engineering/05-published-language.md SOP。

pub(crate) const LOG_TARGET: &str = "aemeath:agent:runtime";

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量.
pub(crate) mod adapters;
pub(crate) mod application;
pub mod composition;
pub(crate) mod domain;
pub(crate) mod ports;

pub use adapters::tool_result_blob::AtomicBlobToolResultStore;
pub use application::run::active_registry::{wire_active_run_registry, ActiveRunRegistry};
pub use application::tool::tool_result_materializer::{
    ToolResultMaterializationPolicyData, ToolResultMaterializer,
};

pub use application::client::{
    config_snapshot_to_sdk, resolve_concurrency_limits, resolve_model_runtime_settings,
    resume_session_to_backing, wire_agent_client_from_args, wire_agent_runner, AgentClientImpl,
    AgentRunnerAssemblyData, CompactModelResolver, InitialProviderAssemblyData,
    ModelRuntimeSettingsData, PromptAssemblyData, RuntimeBootstrapDependenciesData,
    RuntimeCoreDependenciesData, RuntimeToolAssemblyDependenciesData, SessionBootstrapAssemblyData,
    SessionModelSlotData, SkillBootstrapAssemblyData,
};
pub use application::compact_generator::ProviderCompactGenerator;
// #1248: RuntimeContextFactory is the narrow crate-root construction entry.
// RuntimeServices stays internal; callers construct via RuntimeContextFactory::new(…).
pub use application::prompt::build::{build_system_prompt_parts, PromptContextData};
pub use application::prompt::prompt_build_ext::build_static_prompt;
pub use application::reflection::{
    CompleteReflectionResult, ReflectionTaskAdapter, ReflectionTaskCompletionStatus,
    ReflectionTaskRequest, ReflectionTaskSubmitOutcome, ReflectionTaskTrigger,
};
pub use application::run::context::ParentRunContextSource;
pub use application::run::context_factory::RuntimeContextFactory;
pub use ports::{
    ProviderBindingData, ProviderBuildSpecData, ProviderFactory, ProviderPort, ToolResultBlobPort,
    UnavailableUsageSink, UsageSink,
};

#[cfg(test)]
mod boundary_tests {
    use std::path::Path;

    #[test]
    fn application_top_level_modules_have_stable_owners() {
        let application = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/application");
        let allowed = [
            "activity",
            "client",
            "compact_generator",
            "context",
            "hook",
            "interaction",
            "loop_engine",
            "model",
            "prompt",
            "published_state",
            "reflection",
            "run",
            "session",
            "tool",
        ];
        let mut unexpected = std::fs::read_dir(&application)
            .expect("read Runtime application directory")
            .filter_map(|entry| {
                let path = entry.expect("read Runtime application entry").path();
                let file_name = path.file_name()?.to_str()?;
                let module_name = if path.is_dir() {
                    file_name
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    let stem = path.file_stem()?.to_str()?;
                    if stem.ends_with("_tests") {
                        return None;
                    }
                    stem
                } else {
                    return None;
                };
                (!allowed.contains(&module_name)).then(|| file_name.to_string())
            })
            .collect::<Vec<_>>();
        unexpected.sort();

        assert_eq!(
            unexpected,
            Vec::<String>::new(),
            "Runtime application modules must belong to a stable capability owner"
        );
    }

    #[test]
    fn runtime_source_does_not_name_task_persistence_or_legacy_projection() {
        fn assert_tree(path: &Path) {
            for entry in std::fs::read_dir(path).expect("read Runtime source tree") {
                let path = entry.expect("read Runtime source entry").path();
                if path.is_dir() {
                    assert_tree(&path);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    let stem = path
                        .file_stem()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default();
                    if stem.ends_with("_tests")
                        || path
                            .components()
                            .any(|component| component.as_os_str() == "tests")
                    {
                        continue;
                    }
                    let source = std::fs::read_to_string(&path).expect("read Runtime source file");
                    assert!(
                        !source.contains(&["TaskData", "Persist"].concat()),
                        "{} must not name the TaskData persistence capability",
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
