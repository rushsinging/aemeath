use super::*;
use crate::domain::memory_source::MemoryPortSource;
use crate::domain::scope_profile::is_authorized;
use std::collections::BTreeSet;
use std::sync::Arc;
use task::TaskStore;

/// Test-only source that returns a fresh empty in-memory port.
fn test_memory_source() -> Arc<dyn MemoryPortSource> {
    struct TestSource;
    impl MemoryPortSource for TestSource {
        fn current(&self) -> Arc<dyn memory::api::MemoryPort> {
            Arc::new(
                memory::api::InMemoryMemory::new(memory::api::MemoryPolicy::default())
                    .expect("valid default policy"),
            )
        }
    }
    Arc::new(TestSource)
}

fn assembled_scope(scope: BuiltinRegistryScope) -> RegistryScope {
    let registry = ToolRegistry::new();
    let task_access: Arc<dyn TaskAccess> = Arc::new(TaskStore::new());
    let workspace = tempfile::tempdir().expect("workspace");
    let control = project::wire_production_workspace(workspace.path().to_path_buf(), None)
        .expect("workspace wiring")
        .control();
    register_named_scope(
        &registry,
        BuiltinRegistryWiring {
            task_access,
            memory_source: test_memory_source(),
            workspace_control: control,
            skill_loader: crate::composition::wire_skills().loader(),
            skill_catalog: crate::composition::wire_skills().catalog(),
            background_source: test_background_source(),
            workflow_backend: None,
        },
        scope,
    )
}

fn names_for(scope: BuiltinRegistryScope) -> BTreeSet<String> {
    assembled_scope(scope)
        .iter()
        .map(|spec| spec.name().normalized().to_owned())
        .collect()
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| name.to_ascii_lowercase()).collect()
}

/// 空后台进程端口（注册期占位；查询工具装配的真实实现由 runtime 注入）。
fn empty_background_access(
) -> std::sync::Arc<dyn crate::domain::background_process_port::BackgroundProcessAccess> {
    struct EmptyAccess;
    impl crate::domain::background_process_port::BackgroundProcessAccess for EmptyAccess {
        fn list_tasks(
            &self,
        ) -> Vec<crate::domain::types::background_processes::BackgroundProcessSummaryData> {
            Vec::new()
        }
        fn task_status(
            &self,
            _task_id: &str,
        ) -> Option<crate::domain::types::background_processes::BackgroundProcessDetailData>
        {
            None
        }
        fn read_task_log(
            &self,
            _task_id: &str,
            _cursor: Option<u64>,
            _max_bytes: usize,
        ) -> Option<crate::domain::types::background_processes::BackgroundProcessLogData> {
            None
        }
        fn stop_task(
            &self,
            _task_id: &str,
        ) -> Result<crate::domain::types::background_processes::BackgroundProcessStopData, String>
        {
            Err("background processes unavailable".to_string())
        }
    }
    std::sync::Arc::new(EmptyAccess)
}

fn test_background_source(
) -> std::sync::Arc<dyn crate::domain::background_process_port::BackgroundProcessAccessSource> {
    struct FixedSource(
        std::sync::Arc<dyn crate::domain::background_process_port::BackgroundProcessAccess>,
    );
    impl crate::domain::background_process_port::BackgroundProcessAccessSource for FixedSource {
        fn current(
            &self,
        ) -> std::sync::Arc<dyn crate::domain::background_process_port::BackgroundProcessAccess>
        {
            self.0.clone()
        }
    }
    std::sync::Arc::new(FixedSource(empty_background_access()))
}

const FULL: &[&str] = &[
    "Bash",
    "Read",
    "Write",
    "Edit",
    "Glob",
    "Grep",
    "WebFetch",
    "WebSearch",
    "Agent",
    "TaskCreate",
    "TaskUpdate",
    "TaskBlockBy",
    "TaskListGet",
    "TaskLists",
    "TaskListCreate",
    "TaskListComplete",
    "TaskGet",
    "TaskStop",
    "MemoryAdd",
    "MemorySearch",
    "MemoryList",
    "MemoryUpdate",
    "MemoryDelete",
    "AskUserQuestion",
    "ToolSearch",
    "EnterWorktree",
    "ExitWorktree",
    "Skill",
    "BackgroundProcessList",
    "BackgroundProcessStatus",
    "BackgroundProcessLogs",
    "BackgroundProcessStop",
    "BackgroundProcessWatch",
];
#[test]
fn production_profiles_are_main_baseline_or_restricted_children() {
    let main = ToolProfile::baseline(Caps::all());
    let main_profile = profile_for(BuiltinRegistryScope::Main, &main);
    assert_eq!(main_profile.allowed_capabilities(), Caps::all());

    let child = profile_for(BuiltinRegistryScope::SubAgent, &main);
    assert!(child
        .allowed_capabilities()
        .is_subset_of(main.allowed_capabilities()));
    assert_ne!(child.allowed_capabilities(), main.allowed_capabilities());
}

#[test]
fn side_effect_capability_characterization_matches_builtin_behavior() {
    let main_scope = assembled_scope(BuiltinRegistryScope::Main);
    for name in ["TaskGet", "TaskListGet", "TaskLists"] {
        let spec = main_scope
            .get(&crate::domain::published_language::ToolName::new(name))
            .unwrap();
        assert_eq!(spec.required_capabilities(), Caps::TaskRead);
    }
    for name in [
        "TaskCreate",
        "TaskUpdate",
        "TaskBlockBy",
        "TaskListCreate",
        "TaskListComplete",
        "TaskStop",
    ] {
        let spec = main_scope
            .get(&crate::domain::published_language::ToolName::new(name))
            .unwrap();
        assert_eq!(spec.required_capabilities(), Caps::TaskWrite);
    }
}

#[test]
fn registry_exposes_five_memory_tools_and_no_legacy_memory() {
    let registry = ToolRegistry::new();
    let task_access: Arc<dyn TaskAccess> = Arc::new(TaskStore::new());
    let workspace = tempfile::tempdir().expect("workspace");
    let control = project::wire_production_workspace(workspace.path().to_path_buf(), None)
        .expect("workspace wiring")
        .control();
    register_named_scope(
        &registry,
        BuiltinRegistryWiring {
            task_access,
            memory_source: test_memory_source(),
            workspace_control: control,
            skill_loader: crate::composition::wire_skills().loader(),
            skill_catalog: crate::composition::wire_skills().catalog(),
            background_source: test_background_source(),
            workflow_backend: None,
        },
        BuiltinRegistryScope::Main,
    );
    for name in [
        "MemoryAdd",
        "MemorySearch",
        "MemoryList",
        "MemoryUpdate",
        "MemoryDelete",
    ] {
        assert!(registry.get(name).is_some(), "missing {name}");
    }
    assert!(
        registry.get("Memory").is_none(),
        "legacy Memory tool must be gone"
    );
}

/// i18n 是 description 的唯一真相源：ToolSearch 发现通道（`description()`）与
/// LLM 注入通道（`description_for`）在英文下必须逐字一致，否则将来改 i18n
/// 忘了改 adapter 会静默漂移。未覆盖 `description_for` 的工具（如 Skill）经
/// domain 默认回落天然相等，断言同样成立，无需豁免。
#[test]
fn every_tool_description_matches_its_english_i18n_text() {
    let registry = ToolRegistry::new();
    let task_access: Arc<dyn TaskAccess> = Arc::new(TaskStore::new());
    let workspace = tempfile::tempdir().expect("workspace");
    let control = project::wire_production_workspace(workspace.path().to_path_buf(), None)
        .expect("workspace wiring")
        .control();
    register_named_scope(
        &registry,
        BuiltinRegistryWiring {
            task_access,
            memory_source: test_memory_source(),
            workspace_control: control,
            skill_loader: crate::composition::wire_skills().loader(),
            skill_catalog: crate::composition::wire_skills().catalog(),
            background_source: test_background_source(),
            workflow_backend: None,
        },
        BuiltinRegistryScope::Main,
    );

    // 覆盖率护栏：内置全量名单里的每个工具都必须被本断言扫到。
    let checked: BTreeSet<String> = registry.names().into_iter().collect();
    assert!(
        set(FULL).is_subset(&checked),
        "guard must cover every builtin tool"
    );

    for name in registry.names() {
        let tool = registry.get(&name).expect("registered tool");
        assert_eq!(
            tool.description(),
            tool.description_for("en"),
            "tool {name} has a divergent hard-coded description"
        );
    }
}

#[test]
fn retired_lsp_is_absent_from_all_builtin_scopes() {
    for scope in [BuiltinRegistryScope::Main, BuiltinRegistryScope::SubAgent] {
        assert!(
            !names_for(scope).contains("lsp"),
            "retired LSP tool leaked into {scope:?} scope"
        );
    }
}

#[test]
fn full_scope_characterization_is_exact() {
    assert_eq!(names_for(BuiltinRegistryScope::Main), set(FULL));
}

#[test]
fn sub_agent_registration_pool_equals_main_pool() {
    // 注册池统一：静态 [main, sub] 布尔退役，两个 scope 注册同一全量名单；
    // 等价迁移由 sub-agent-restricted profile 的显式名单承载。
    assert_eq!(
        names_for(BuiltinRegistryScope::SubAgent),
        names_for(BuiltinRegistryScope::Main),
    );
}

#[test]
fn sub_agent_restricted_profile_assembles_expected_toolset_from_caps() {
    // caps 组装：Read|Write|Execute|NetworkAccess 位挑选的工具组。
    // Memory/ToolSearch 归 All 位（main 专属），默认组装不出。
    let main = ToolProfile::baseline(Caps::all());
    let restricted = profile_for(BuiltinRegistryScope::SubAgent, &main);
    assert_eq!(
        restricted.allowed_capabilities(),
        Caps::Read | Caps::Write | Caps::Execute | Caps::NetworkAccess
    );
    let scope = assembled_scope(BuiltinRegistryScope::SubAgent);
    let mut assembled: Vec<String> = scope
        .iter()
        .filter(|spec| is_authorized(spec, &restricted))
        .map(|spec| spec.name().to_string())
        .collect();
    assembled.sort();
    assert_eq!(
        assembled,
        vec![
            "Bash",
            "Edit",
            "Glob",
            "Grep",
            "Read",
            "Skill",
            "WebFetch",
            "WebSearch",
            "Write",
        ]
    );
}

#[test]
fn sub_agent_scope_characterization_is_exact() {
    let main_names = names_for(BuiltinRegistryScope::Main);
    let sub_agent_names = names_for(BuiltinRegistryScope::SubAgent);

    assert_eq!(sub_agent_names, main_names);
    assert!(main_names.contains("agent"));
    assert!(sub_agent_names.contains("agent"));
    for ordinary_tool in ["read", "grep", "bash", "skill"] {
        assert!(sub_agent_names.contains(ordinary_tool));
    }
}

/// 与 `assembled_scope` 相同，但注入可选 Workflow backend（#2084）。
fn assembled_scope_with_backend(
    scope: BuiltinRegistryScope,
    workflow_backend: Option<Arc<dyn workflow::WorkflowToolBackend>>,
) -> RegistryScope {
    let registry = ToolRegistry::new();
    let task_access: Arc<dyn TaskAccess> = Arc::new(TaskStore::new());
    let workspace = tempfile::tempdir().expect("workspace");
    let control = project::wire_production_workspace(workspace.path().to_path_buf(), None)
        .expect("workspace wiring")
        .control();
    register_named_scope(
        &registry,
        BuiltinRegistryWiring {
            task_access,
            memory_source: test_memory_source(),
            workspace_control: control,
            skill_loader: crate::composition::wire_skills().loader(),
            skill_catalog: crate::composition::wire_skills().catalog(),
            background_source: test_background_source(),
            workflow_backend,
        },
        scope,
    )
}

/// 未注入 backend 时不注册六工具：避免向 LLM 暴露无法执行的工具。
#[test]
fn workflow_tools_absent_without_backend() {
    let names = assembled_scope_with_backend(BuiltinRegistryScope::Main, None)
        .iter()
        .map(|spec| spec.name().normalized().to_owned())
        .collect::<BTreeSet<String>>();
    for name in [
        "workflow_create",
        "workflow_list",
        "workflow_query",
        "workflow_update",
        "workflow_node_update",
        "workflow_replan",
    ] {
        assert!(
            !names.contains(name),
            "{name} must not be registered without backend"
        );
    }
}

/// 注入 backend 时六个 Workflow Tool 全部出现在 catalog scope。
#[test]
fn workflow_tools_registered_with_backend() {
    struct UnusedBackend;
    #[async_trait::async_trait]
    impl workflow::WorkflowToolBackend for UnusedBackend {
        async fn create(
            &self,
            _spec: workflow::CreateWorkflowSpec,
        ) -> Result<workflow::WorkflowSummaryData, workflow::WorkflowToolError> {
            Err(workflow::WorkflowToolError::WorkflowNotVisible {
                workflow_id: "unused".to_owned(),
            })
        }
        async fn list(
            &self,
            _query: workflow::ListWorkflowsQuery,
        ) -> Result<workflow::ListWorkflowsData, workflow::WorkflowToolError> {
            Err(workflow::WorkflowToolError::WorkflowNotVisible {
                workflow_id: "unused".to_owned(),
            })
        }
        async fn query(
            &self,
            _query: workflow::QueryWorkflowQuery,
        ) -> Result<workflow::WorkflowDetailData, workflow::WorkflowToolError> {
            Err(workflow::WorkflowToolError::WorkflowNotVisible {
                workflow_id: "unused".to_owned(),
            })
        }
        async fn update(
            &self,
            _command: workflow::UpdateWorkflowCommand,
        ) -> Result<workflow::LifecycleResultData, workflow::WorkflowToolError> {
            Err(workflow::WorkflowToolError::WorkflowNotVisible {
                workflow_id: "unused".to_owned(),
            })
        }
        async fn node_update(
            &self,
            _command: workflow::UpdateNodeCommand,
        ) -> Result<workflow::NodeUpdateResultData, workflow::WorkflowToolError> {
            Err(workflow::WorkflowToolError::WorkflowNotVisible {
                workflow_id: "unused".to_owned(),
            })
        }
        async fn replan(
            &self,
            _command: workflow::ReplanWorkflowCommand,
        ) -> Result<workflow::ReplanResultData, workflow::WorkflowToolError> {
            Err(workflow::WorkflowToolError::WorkflowNotVisible {
                workflow_id: "unused".to_owned(),
            })
        }
    }

    let scope =
        assembled_scope_with_backend(BuiltinRegistryScope::Main, Some(Arc::new(UnusedBackend)));
    let names = scope
        .iter()
        .map(|spec| spec.name().normalized().to_owned())
        .collect::<BTreeSet<String>>();
    for name in [
        "workflow_create",
        "workflow_list",
        "workflow_query",
        "workflow_update",
        "workflow_node_update",
        "workflow_replan",
    ] {
        assert!(
            names.contains(name),
            "{name} must be registered with backend"
        );
    }
    // 六个工具归 All 位（main 专属），与 Memory 同档。
    for spec in scope
        .iter()
        .filter(|spec| spec.name().normalized().starts_with("workflow_"))
    {
        assert_eq!(
            spec.required_capabilities(),
            crate::domain::published_language::ToolCapabilities::All,
            "{} must require Caps::All",
            spec.name().normalized()
        );
    }
}
