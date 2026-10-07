use std::sync::Arc;

use sdk::SdkError;
use share::config::models::ResolvedModel;

use crate::application::client::bootstrap::{ChatBootstrapArgs, ModelRuntimeSettingsData};
use crate::ports::ProviderFactory;

use super::accessors::{AgentClientImpl, RuntimeHandle};

/// 由 Composition 装配、供 Runtime bootstrap 转发的 Tool/Skill/Run 资源。
pub struct RuntimeToolAssemblyDependenciesData {
    tool_catalog: Arc<dyn tools::ToolCatalogPort>,
    skill_catalog: Arc<dyn tools::published::skill::SkillCatalogPort>,
    tool_result_materializer:
        Arc<crate::application::tool::tool_result_materializer::ToolResultMaterializer>,
    active_run: Arc<crate::application::run::active_registry::ActiveRunRegistry>,
    /// #252 PR3：后台任务端口绑定槽（session 创建后写入实现）。
    background_slot: Option<
        std::sync::Arc<std::sync::RwLock<Option<std::sync::Arc<dyn tools::BackgroundTaskAccess>>>>,
    >,
}

impl RuntimeToolAssemblyDependenciesData {
    pub fn new(
        tool_catalog: Arc<dyn tools::ToolCatalogPort>,
        skill_catalog: Arc<dyn tools::published::skill::SkillCatalogPort>,
        tool_result_materializer: Arc<
            crate::application::tool::tool_result_materializer::ToolResultMaterializer,
        >,
        active_run: Arc<crate::application::run::active_registry::ActiveRunRegistry>,
    ) -> Self {
        Self {
            tool_catalog,
            skill_catalog,
            tool_result_materializer,
            active_run,
            background_slot: None,
        }
    }

    /// 绑定后台任务端口槽（#252 PR3：shell 构造后写入实现）。
    pub fn with_background_slot(
        mut self,
        slot: std::sync::Arc<
            std::sync::RwLock<Option<std::sync::Arc<dyn tools::BackgroundTaskAccess>>>,
        >,
    ) -> Self {
        self.background_slot = Some(slot);
        self
    }
}

/// 由 Composition 装配、供 Runtime bootstrap 转发的基础运行资源。
pub struct RuntimeCoreDependenciesData {
    workspace: project::Workspace,
    wiring: Arc<context::MainSessionWiring>,
    provider_factory: Arc<dyn ProviderFactory>,
    session_management: Arc<dyn context::SessionManagementPort>,
}

impl RuntimeCoreDependenciesData {
    pub fn new(
        workspace: project::Workspace,
        wiring: Arc<context::MainSessionWiring>,
        provider_factory: Arc<dyn ProviderFactory>,
        session_management: Arc<dyn context::SessionManagementPort>,
    ) -> Self {
        Self {
            workspace,
            wiring,
            provider_factory,
            session_management,
        }
    }
}

pub struct SessionBootstrapAssemblyData {
    pub cwd: std::path::PathBuf,
    pub context_size: usize,
    pub allow_all: bool,
    pub verbose: bool,
    pub resume: Option<String>,
}

impl SessionBootstrapAssemblyData {
    pub fn new(
        cwd: std::path::PathBuf,
        context_size: usize,
        allow_all: bool,
        verbose: bool,
        resume: Option<String>,
    ) -> Self {
        Self {
            cwd,
            context_size,
            allow_all,
            verbose,
            resume,
        }
    }
}

pub struct SkillBootstrapAssemblyData {
    pub snapshot: tools::published::skill::SkillCatalogSnapshot,
    /// 轮次边界重扫组件：会话中 skill 文件变更后经 `SkillsUpdated`
    /// 事件刷新 TUI slash 目录（初始 revision 即 snapshot）。
    pub refresh: crate::application::client::SkillCatalogRefresh,
}

impl SkillBootstrapAssemblyData {
    pub fn new(
        catalog: std::sync::Arc<dyn tools::published::skill::SkillCatalogPort>,
        workspace: project::Workspace,
        query: tools::published::skill::SkillQuery,
    ) -> Self {
        let snapshot = tools::published::skill::SkillCatalogSnapshot::from_descriptors(
            catalog.list(query.clone()),
        );
        let refresh = crate::application::client::SkillCatalogRefresh::new(
            catalog, workspace, query, &snapshot,
        );
        Self { snapshot, refresh }
    }
}

pub struct PromptAssemblyData {
    pub system_blocks: Vec<crate::ports::RequestSystemBlockData>,
    pub system_prompt_text: String,
    pub initial_git_context: String,
    pub user_context: String,
    pub model_id: String,
}

impl PromptAssemblyData {
    pub fn new(
        system_blocks: Vec<crate::ports::RequestSystemBlockData>,
        initial_git_context: String,
        user_context: String,
        model_id: impl Into<String>,
    ) -> Self {
        let system_prompt_text = system_blocks
            .iter()
            .map(crate::ports::RequestSystemBlockData::text)
            .collect::<Vec<_>>()
            .join("\n\n");
        Self {
            system_blocks,
            system_prompt_text,
            initial_git_context,
            user_context,
            model_id: model_id.into(),
        }
    }
}

pub struct InitialProviderAssemblyData {
    binding: crate::ports::ProviderBindingData,
    resolved_model: ResolvedModel,
    runtime_settings: ModelRuntimeSettingsData,
    compact_model_slot: crate::application::client::SessionModelSlotData,
}

impl InitialProviderAssemblyData {
    pub fn new(
        binding: crate::ports::ProviderBindingData,
        resolved_model: ResolvedModel,
        runtime_settings: ModelRuntimeSettingsData,
        compact_model_slot: crate::application::client::SessionModelSlotData,
    ) -> Self {
        Self {
            binding,
            resolved_model,
            runtime_settings,
            compact_model_slot,
        }
    }

    pub fn binding(&self) -> &crate::ports::ProviderBindingData {
        &self.binding
    }

    pub fn resolved_model(&self) -> &ResolvedModel {
        &self.resolved_model
    }

    pub fn runtime_settings(&self) -> &ModelRuntimeSettingsData {
        &self.runtime_settings
    }

    /// Compact 模型解析共享的会话模型槽；Composition 用它构造解析器。
    pub fn compact_model_slot(&self) -> crate::application::client::SessionModelSlotData {
        self.compact_model_slot.clone()
    }
}

pub struct RuntimeIngressAssembly {
    pub(crate) event_sink_factory: Arc<super::accessors::EventSinkFactory>,
    pub(crate) input_port_factory: Arc<super::accessors::InputPortFactory>,
}

impl RuntimeIngressAssembly {
    pub(crate) fn new(
        event_sink_factory: Arc<super::accessors::EventSinkFactory>,
        input_port_factory: Arc<super::accessors::InputPortFactory>,
    ) -> Self {
        Self {
            event_sink_factory,
            input_port_factory,
        }
    }
}

/// Runtime bootstrap 所需的活依赖；由 Composition 一次性构造并注入。
///
/// `runtime_context_factory` 随 Agent Runner assembly 进入 bootstrap，保证
/// Main 与 Derived 路径共享同一基础 factory 实例。
pub struct RuntimeBootstrapDependenciesData {
    /// 核心依赖（workspace/会话/provider 工厂）。
    core: RuntimeCoreDependenciesData,
    /// 工具装配依赖（目录/物化器/活动注册表）。
    tool_assembly: RuntimeToolAssemblyDependenciesData,
    /// agent runner 装配（runner/并发/上下文工厂）。
    agent_runner: crate::application::client::bootstrap::AgentRunnerAssemblyData,
    ingress: RuntimeIngressAssembly,
    initial_provider: InitialProviderAssemblyData,
    session_bootstrap: SessionBootstrapAssemblyData,
    prompt: PromptAssemblyData,
    skills: SkillBootstrapAssemblyData,
}

impl RuntimeBootstrapDependenciesData {
    pub fn new(
        core: RuntimeCoreDependenciesData,
        tool_assembly: RuntimeToolAssemblyDependenciesData,
        ingress: RuntimeIngressAssembly,
        initial_provider: InitialProviderAssemblyData,
        session_bootstrap: SessionBootstrapAssemblyData,
        prompt: PromptAssemblyData,
        skills: SkillBootstrapAssemblyData,
        agent_runner: crate::application::client::bootstrap::AgentRunnerAssemblyData,
    ) -> Self {
        let RuntimeToolAssemblyDependenciesData { active_run, .. } = &tool_assembly;
        let crate::application::client::bootstrap::AgentRunnerAssemblyData {
            active_run: agent_runner_active_run,
            ..
        } = &agent_runner;
        assert!(
            Arc::ptr_eq(
                &(active_run.clone() as Arc<dyn crate::domain::agent_run::ActiveRunPort>),
                agent_runner_active_run,
            ),
            "Main Runtime 与 Derived Agent Runner 必须共享同一 ActiveRun 控制面",
        );
        Self {
            core,
            tool_assembly,
            agent_runner,
            ingress,
            initial_provider,
            session_bootstrap,
            prompt,
            skills,
        }
    }

    pub fn runtime_context_factory(
        &self,
    ) -> &Arc<crate::application::run::context_factory::RuntimeContextFactory> {
        self.agent_runner.runtime_context_factory()
    }

    pub fn session_management(&self) -> Arc<dyn context::SessionManagementPort> {
        self.core.session_management.clone()
    }

    pub fn wiring(&self) -> Arc<context::MainSessionWiring> {
        self.core.wiring.clone()
    }

    pub fn tool_catalog(&self) -> Arc<dyn tools::ToolCatalogPort> {
        self.tool_assembly.tool_catalog.clone()
    }

    pub fn skill_catalog(&self) -> Arc<dyn tools::published::skill::SkillCatalogPort> {
        self.tool_assembly.skill_catalog.clone()
    }

    pub fn tool_result_materializer(
        &self,
    ) -> Arc<crate::application::tool::tool_result_materializer::ToolResultMaterializer> {
        self.tool_assembly.tool_result_materializer.clone()
    }

    pub fn active_run(&self) -> Arc<crate::application::run::active_registry::ActiveRunRegistry> {
        self.tool_assembly.active_run.clone()
    }
}

/// 从 Args 初始化 AgentClient。
///
/// 模型选择直接使用 `Config.models.select_for_run()`，无需外部注入。
///
/// `task_access` 由 Composition 层注入；Runtime 不得自行创建
/// TaskData BC 的 backing 或持久化封套（跨域越权，#890）。
pub async fn wire_agent_client_from_args(
    _args: ChatBootstrapArgs,
    dependencies: RuntimeBootstrapDependenciesData,
) -> Result<AgentClientImpl, SdkError> {
    let RuntimeBootstrapDependenciesData {
        core,
        tool_assembly,
        ingress,
        initial_provider,
        session_bootstrap,
        prompt,
        skills,
        agent_runner: runner_assembly,
    } = dependencies;
    let RuntimeCoreDependenciesData {
        workspace,
        wiring,
        provider_factory,
        session_management,
    } = core;
    let RuntimeToolAssemblyDependenciesData {
        tool_catalog: _,
        skill_catalog,
        tool_result_materializer,
        active_run,
        background_slot,
    } = tool_assembly;
    let crate::application::client::bootstrap::AgentRunnerAssemblyData {
        runner: agent_runner,
        parent_context_source,
        max_tool_concurrency,
        max_agent_concurrency,
        agent_semaphore,
        runtime_context_factory,
        ..
    } = runner_assembly;
    let _ = &session_management;

    // Config query/writer come from the wiring gate-aware façade.
    // Bootstrap reads committed_config directly from wiring (one-shot).
    let config_query = wiring.config_query();
    let config_writer = wiring.config_writer();

    let SessionBootstrapAssemblyData {
        cwd,
        context_size,
        allow_all,
        verbose,
        resume,
    } = session_bootstrap;

    // 3. Session — startup resume is scoped to the current project identity.
    // A rejected cross-project id leaves the committed snapshot unchanged.
    // 职责 1（resume 解析与 SDK backing 映射）由 startup_resume 模块承担。
    let (session_id, startup_resume) =
        super::startup_resume::resolve_startup_session(resume.as_deref(), &wiring).await?;
    // Session id determined above; committed_config remains bound to the
    // current project because cross-project resume is rejected.

    // 3b. SessionStart 生命周期 hook：会话身份确定（新会话或 --resume）后 emit，
    // 外部集成（如终端会话恢复）据此捕获可 resume 的会话 id。
    // 生命周期点非闸门：hook 失败不阻断启动。
    crate::application::hook::session_start::emit_session_start(
        &runtime_context_factory.services().hooks,
        &cwd,
        &session_id,
    )
    .await;

    // 4. Read the current committed config snapshot.
    let snapshot = wiring.committed_config();

    // 5. 日志已由 Composition 在进入 Runtime 前初始化。

    // 6. 初始模型绑定由 Composition 解析并构造；Runtime 只消费 typed assembly。
    let InitialProviderAssemblyData {
        binding,
        resolved_model,
        runtime_settings: _,
        compact_model_slot,
    } = initial_provider;
    // Compact 模型解析所需的会话模型真相源：Composition 创建的槽在这里绑定，
    // 之后 `/model` 切换与 compact 解析共享同一 `SessionModelState`。
    let model_state = crate::application::client::SessionModelState::new(
        resolved_model.clone(),
        Arc::new(binding.clone()),
    );
    compact_model_slot.bind(model_state.clone());

    // Tool and Skill bootstrap results are assembled and frozen by Composition.
    let SkillBootstrapAssemblyData {
        snapshot: initial_skill_snapshot,
        refresh: skill_refresh,
    } = skills;
    // #1327 承接 MCP Ready lifecycle / Catalog 同步；#1294 不保留 MCP manager 或
    // Tools 私有 CatalogExecutionWiring 接线。

    // 12. Hook runner 由 Composition 注入，Main/Sub 共享同一实例。

    // 13. Tool Result materializer 与 14. active-run registry 由 Composition 注入。

    // Concurrency settings and shared semaphore are frozen by Composition.

    // 16. Policy 已由 Composition 注入；同一 Arc 分发给 Main 与 Sub。

    // 17. #1385 TaskData 7: Memory port is obtained per-run via BoundMainRun
    // (assemble_main_runtime_context), not at bootstrap time.

    // Parent context source and concrete AgentRunner are assembled by Composition.

    // Prompt content is assembled by Composition and frozen for this session.
    let PromptAssemblyData {
        system_blocks,
        system_prompt_text,
        initial_git_context,
        user_context,
        model_id,
    } = prompt;

    // 19. Concurrency
    log::info!(
        target: crate::LOG_TARGET,
        "concurrency limits: max_tool={}, max_agent={}",
        max_tool_concurrency,
        max_agent_concurrency
    );

    let memory_config = snapshot.memory().clone();

    // 20b. 构建统一 SessionRuntime（session 级状态，§2.2）
    let shell = crate::application::client::accessors::SessionRuntime::new(
        Arc::new(std::sync::RwLock::new(
            crate::application::run::creation::SessionState::new(
                session_id.clone(),
                cwd.clone(),
                format!("{}/{}", binding.model.provider, binding.model.model),
                snapshot.clone(),
            ),
        )),
        workspace.clone(),
        wiring.clone(),
        config_query.clone(),
        config_writer.clone(),
        session_management.clone(),
        provider_factory.clone(),
        model_state,
        max_tool_concurrency,
        max_agent_concurrency,
        agent_semaphore.clone(),
        system_blocks,
        system_prompt_text,
        initial_git_context,
        user_context,
        model_id,
        skill_catalog,
        initial_skill_snapshot,
        skill_refresh,
        memory_config,
        context_size,
        snapshot.language().to_string(),
        allow_all,
        verbose,
        resume,
        startup_resume,
        agent_runner,
        parent_context_source,
        tool_result_materializer,
        active_run.clone(),
        ingress.event_sink_factory,
        ingress.input_port_factory,
        runtime_context_factory,
    );

    // #252 PR3：session 就绪后绑定后台任务端口（查询 tool 的数据源）。
    if let Some(slot) = background_slot {
        slot.write()
            .unwrap_or_else(|error| error.into_inner())
            .replace(shell.background_tasks.clone());
    }

    // 21. 构建 handle — #1385 TaskData 7: shell is the single source.
    let handle = RuntimeHandle { shell };

    Ok(AgentClientImpl {
        inner: Arc::new(handle),
    })
}

#[cfg(test)]
#[path = "from_args_tests.rs"]
mod tests;
