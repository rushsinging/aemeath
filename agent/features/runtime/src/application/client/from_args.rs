use std::sync::Arc;

use sdk::SdkError;
use share::config::models::ResolvedModel;

use crate::application::client::bootstrap::{ChatBootstrapArgs, ModelRuntimeSettings};
use crate::ports::ProviderFactory;

use super::accessors::{AgentClientImpl, RuntimeHandle};

/// 由 Composition 装配、供 Runtime bootstrap 转发的 Tool/Skill/Run 资源。
pub struct RuntimeToolAssemblyDependencies {
    tool_catalog: Arc<dyn tools::ToolCatalogPort>,
    skill_catalog: Arc<dyn tools::SkillCatalogPort>,
    tool_result_materializer:
        Arc<crate::application::tool::tool_result_materializer::ToolResultMaterializer>,
    active_run: Arc<crate::application::run::active_registry::ActiveRunRegistry>,
}

impl RuntimeToolAssemblyDependencies {
    pub fn new(
        tool_catalog: Arc<dyn tools::ToolCatalogPort>,
        skill_catalog: Arc<dyn tools::SkillCatalogPort>,
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
        }
    }
}

/// 由 Composition 装配、供 Runtime bootstrap 转发的基础运行资源。
pub struct RuntimeCoreDependencies {
    workspace: project::WorkspaceViews,
    wiring: Arc<context::MainSessionWiring>,
    provider_factory: Arc<dyn ProviderFactory>,
    session_management: Arc<dyn context::SessionManagementPort>,
}

impl RuntimeCoreDependencies {
    pub fn new(
        workspace: project::WorkspaceViews,
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

pub struct SessionBootstrapAssembly {
    pub cwd: std::path::PathBuf,
    pub context_size: usize,
    pub allow_all: bool,
    pub verbose: bool,
    pub resume: Option<String>,
}

impl SessionBootstrapAssembly {
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

pub struct SkillBootstrapAssembly {
    pub snapshot: tools::SkillCatalogSnapshot,
}

impl SkillBootstrapAssembly {
    pub fn new(snapshot: tools::SkillCatalogSnapshot) -> Self {
        Self { snapshot }
    }
}

pub struct PromptAssembly {
    pub system_blocks: Vec<crate::ports::RequestSystemBlock>,
    pub system_prompt_text: String,
    pub initial_git_context: String,
    pub user_context: String,
}

impl PromptAssembly {
    pub fn new(
        system_blocks: Vec<crate::ports::RequestSystemBlock>,
        initial_git_context: String,
        user_context: String,
    ) -> Self {
        let system_prompt_text = system_blocks
            .iter()
            .map(crate::ports::RequestSystemBlock::text)
            .collect::<Vec<_>>()
            .join("\n\n");
        Self {
            system_blocks,
            system_prompt_text,
            initial_git_context,
            user_context,
        }
    }
}

pub struct InitialProviderAssembly {
    binding: crate::ports::ProviderBinding,
    resolved_model: ResolvedModel,
    runtime_settings: ModelRuntimeSettings,
}

impl InitialProviderAssembly {
    pub fn new(
        binding: crate::ports::ProviderBinding,
        resolved_model: ResolvedModel,
        runtime_settings: ModelRuntimeSettings,
    ) -> Self {
        Self {
            binding,
            resolved_model,
            runtime_settings,
        }
    }

    pub fn binding(&self) -> &crate::ports::ProviderBinding {
        &self.binding
    }

    pub fn resolved_model(&self) -> &ResolvedModel {
        &self.resolved_model
    }

    pub fn runtime_settings(&self) -> &ModelRuntimeSettings {
        &self.runtime_settings
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
pub struct RuntimeBootstrapDependencies {
    workspace: project::WorkspaceViews,
    wiring: Arc<context::MainSessionWiring>,
    provider_factory: Arc<dyn ProviderFactory>,
    session_management: Arc<dyn context::SessionManagementPort>,
    tool_catalog: Arc<dyn tools::ToolCatalogPort>,
    skill_catalog: Arc<dyn tools::SkillCatalogPort>,
    tool_result_materializer:
        Arc<crate::application::tool::tool_result_materializer::ToolResultMaterializer>,
    active_run: Arc<crate::application::run::active_registry::ActiveRunRegistry>,
    ingress: RuntimeIngressAssembly,
    initial_provider: InitialProviderAssembly,
    session_bootstrap: SessionBootstrapAssembly,
    prompt: PromptAssembly,
    skills: SkillBootstrapAssembly,
    agent_runner: Arc<dyn tools::AgentRunner>,
    parent_context_source: crate::application::run::context::ParentRunContextSource,
    max_tool_concurrency: usize,
    max_agent_concurrency: usize,
    agent_semaphore: Arc<tokio::sync::Semaphore>,
    runtime_context_factory: Arc<crate::application::run::context_factory::RuntimeContextFactory>,
}

impl RuntimeBootstrapDependencies {
    pub fn new(
        core: RuntimeCoreDependencies,
        tool_assembly: RuntimeToolAssemblyDependencies,
        ingress: RuntimeIngressAssembly,
        initial_provider: InitialProviderAssembly,
        session_bootstrap: SessionBootstrapAssembly,
        prompt: PromptAssembly,
        skills: SkillBootstrapAssembly,
        agent_runner: crate::application::client::bootstrap::AgentRunnerAssembly,
    ) -> Self {
        let RuntimeCoreDependencies {
            workspace,
            wiring,
            provider_factory,
            session_management,
        } = core;
        let RuntimeToolAssemblyDependencies {
            tool_catalog,
            skill_catalog,
            tool_result_materializer,
            active_run,
            ..
        } = tool_assembly;
        let crate::application::client::bootstrap::AgentRunnerAssembly {
            runner: agent_runner,
            parent_context_source,
            active_run: agent_runner_active_run,
            max_tool_concurrency,
            max_agent_concurrency,
            agent_semaphore,
            runtime_context_factory,
        } = agent_runner;
        assert!(
            Arc::ptr_eq(
                &(active_run.clone() as Arc<dyn crate::domain::agent_run::ActiveRunPort>),
                &agent_runner_active_run,
            ),
            "Main Runtime 与 Derived Agent Runner 必须共享同一 ActiveRun 控制面",
        );
        Self {
            workspace,
            wiring,
            provider_factory,
            session_management,
            tool_catalog,
            skill_catalog,
            tool_result_materializer,
            active_run,
            ingress,
            initial_provider,
            session_bootstrap,
            prompt,
            skills,
            agent_runner,
            parent_context_source,
            max_tool_concurrency,
            max_agent_concurrency,
            agent_semaphore,
            runtime_context_factory,
        }
    }

    pub fn runtime_context_factory(
        &self,
    ) -> &Arc<crate::application::run::context_factory::RuntimeContextFactory> {
        &self.runtime_context_factory
    }

    pub fn session_management(&self) -> Arc<dyn context::SessionManagementPort> {
        self.session_management.clone()
    }

    pub fn wiring(&self) -> Arc<context::MainSessionWiring> {
        self.wiring.clone()
    }

    pub fn tool_catalog(&self) -> Arc<dyn tools::ToolCatalogPort> {
        self.tool_catalog.clone()
    }

    pub fn skill_catalog(&self) -> Arc<dyn tools::SkillCatalogPort> {
        self.skill_catalog.clone()
    }

    pub fn tool_result_materializer(
        &self,
    ) -> Arc<crate::application::tool::tool_result_materializer::ToolResultMaterializer> {
        self.tool_result_materializer.clone()
    }

    pub fn active_run(&self) -> Arc<crate::application::run::active_registry::ActiveRunRegistry> {
        self.active_run.clone()
    }
}

/// 从 Args 初始化 AgentClient。
///
/// 模型选择直接使用 `Config.models.select_for_run()`，无需外部注入。
///
/// `task_access` 由 Composition 层注入；Runtime 不得自行创建
/// Task BC 的 backing 或持久化封套（跨域越权，#890）。
pub async fn from_args_with_workspace(
    _args: ChatBootstrapArgs,
    dependencies: RuntimeBootstrapDependencies,
) -> Result<AgentClientImpl, SdkError> {
    let RuntimeBootstrapDependencies {
        workspace,
        wiring,
        provider_factory,
        session_management,
        tool_catalog: _,
        skill_catalog,
        tool_result_materializer,
        active_run,
        ingress,
        initial_provider,
        session_bootstrap,
        prompt,
        skills,
        agent_runner,
        parent_context_source,
        max_tool_concurrency,
        max_agent_concurrency,
        agent_semaphore,
        runtime_context_factory,
        ..
    } = dependencies;

    // Config query/writer come from the wiring gate-aware façade.
    // Bootstrap reads committed_config directly from wiring (one-shot).
    let config_query = wiring.config_query();
    let config_writer = wiring.config_writer();

    let SessionBootstrapAssembly {
        cwd,
        context_size,
        allow_all,
        verbose,
        resume,
    } = session_bootstrap;

    // 3. Session — startup resume is scoped to the current project identity.
    // A rejected cross-project id leaves the committed snapshot unchanged.
    let (session_id, startup_resume) = if let Some(resume_id) = resume.as_ref() {
        match crate::application::client::resume_helper::resume_session_to_backing(
            resume_id, &wiring,
        )
        .await
        {
            Ok(resume_view) => {
                log::info!(target: crate::LOG_TARGET, "startup resume: {}", resume_view.session_id);
                log::debug!(
                    target: crate::LOG_TARGET,
                    "resume_lifecycle boundary=startup_view stage=view_created session_id={} display_index_steps={} legacy_steps={} active_messages={}",
                    resume_view.session_id,
                    resume_view
                        .display_history
                        .as_ref()
                        .map_or(0, |index| index.steps().len()),
                    resume_view.display_steps.len(),
                    resume_view.active_messages.len(),
                );
                let session_id = resume_view.session_id.clone();
                let startup_resume = sdk::LocalSessionResumeBacking {
                    steps: resume_view
                        .display_steps
                        .into_iter()
                        .map(|step| sdk::LocalResumedSessionStep {
                            run_id: step.run_id,
                            step_id: step.step_id,
                            message_segments: step.message_segments,
                            finalize_cause: step
                                .finalize_cause
                                .map(super::mapping::map_finalize_cause_to_sdk),
                            duration_ms: step.duration_ms,
                        })
                        .collect(),
                    display_history: resume_view.display_history.map(|index| {
                        sdk::DisplayHistoryIndex {
                            session_id: index.session_id().to_string(),
                            generation_revision: index.generation_revision(),
                            steps: index
                                .steps()
                                .iter()
                                .map(|step| sdk::DisplayHistoryStepReference {
                                    run_id: step.run_id().to_string(),
                                    step_id: step.step_id().to_string(),
                                    member_name: step.member_name().to_string(),
                                    estimated_lines: step.estimated_lines(),
                                    user_input_history: step.user_input_history().to_vec(),
                                    finalize_cause: step
                                        .finalize_cause()
                                        .map(super::mapping::map_finalize_cause_to_sdk),
                                    duration_ms: step.duration_ms(),
                                })
                                .collect(),
                        }
                    }),
                    session_id: resume_view.session_id,
                    created_at: chrono::DateTime::parse_from_rfc3339(&resume_view.created_at)
                        .map(|dt| dt.timestamp_millis() as u64)
                        .unwrap_or(0),
                    compacted: resume_view.compacted,
                };
                (session_id, Some(startup_resume))
            }
            Err(error) => {
                return Err(SdkError::Init(format!(
                    "startup resume of session {resume_id} failed: {error}"
                )));
            }
        }
    } else {
        // Non-resume: use the wiring's committed session id so Runtime
        // and the Context coordinator share the same canonical session.
        let session_id = wiring.committed_session().id.clone();
        log::info!(target: crate::LOG_TARGET, "session started");
        (session_id, None)
    };
    // Session id determined above; committed_config remains bound to the
    // current project because cross-project resume is rejected.

    // 4. Read the current committed config snapshot.
    let snapshot = wiring.committed_config();

    // 5. 日志已由 Composition 在进入 Runtime 前初始化。

    // 6. 初始模型绑定由 Composition 解析并构造；Runtime 只消费 typed assembly。
    let InitialProviderAssembly {
        binding,
        resolved_model,
        runtime_settings: _,
    } = initial_provider;

    // Tool and Skill bootstrap results are assembled and frozen by Composition.
    let SkillBootstrapAssembly {
        snapshot: initial_skill_snapshot,
    } = skills;
    // #1327 承接 MCP Ready lifecycle / Catalog 同步；#1294 不保留 MCP manager 或
    // Tools 私有 CatalogExecutionWiring 接线。

    // 12. Hook runner 由 Composition 注入，Main/Sub 共享同一实例。

    // 13. Tool Result materializer 与 14. active-run registry 由 Composition 注入。

    // Concurrency settings and shared semaphore are frozen by Composition.

    // 16. PolicyPort 已由 Composition 注入；同一 Arc 分发给 Main 与 Sub。

    // 17. #1385 Task 7: Memory port is obtained per-run via BoundMainRun
    // (assemble_main_runtime_context), not at bootstrap time.

    // Parent context source and concrete AgentRunner are assembled by Composition.

    // Prompt content is assembled by Composition and frozen for this session.
    let PromptAssembly {
        system_blocks,
        system_prompt_text,
        initial_git_context,
        user_context,
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
        crate::application::client::accessors::SessionModelState::new(
            resolved_model.clone(),
            Arc::new(binding.clone()),
        ),
        max_tool_concurrency,
        max_agent_concurrency,
        agent_semaphore.clone(),
        system_blocks,
        system_prompt_text,
        initial_git_context,
        user_context,
        skill_catalog,
        initial_skill_snapshot,
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

    // 21. 构建 handle — #1385 Task 7: shell is the single source.
    let handle = RuntimeHandle { shell };

    Ok(AgentClientImpl {
        inner: Arc::new(handle),
    })
}

#[cfg(test)]
#[path = "from_args_tests.rs"]
mod tests;
