//! composition 装配：RSI 维护 daemon 的组合根。
//!
//! 任务集（TaskKind 装配）在此唯一装配：本阶段注册空任务集
//! （改进循环实现由后续 issue 接入），daemon 框架面先稳定。

use harness::adapters::daemon_serve::{run_rsi_daemon, RsiDaemonEndpoints};
use harness::domain::task::TaskRegistry;
use harness::ComponentRegistry;
use std::path::{Path, PathBuf};

/// `aemeath rsi serve` 的退出结果。
#[derive(Debug)]
pub enum RsiServeExit {
    Stopped { sock: std::path::PathBuf },
    Failure { message: String, exit_code: i32 },
}

/// 运行 RSI 维护 daemon：装配端点与任务集，阻塞至 Stop/ctrl-c。
pub async fn run_rsi_serve(on_ready: impl FnOnce(&Path)) -> RsiServeExit {
    let agents_dir = share::config::paths::global_agents_dir();
    let endpoints = RsiDaemonEndpoints::under_agents_dir(&agents_dir);
    let sock = endpoints.sock.clone();

    // 真任务集装配（与 run 模式同源）：registry + LLM + 经验库。
    // 装配失败按 serve 失败退出（配置/凭证问题的显式报错优于静默空转）。
    let config = match load_global_config_layered() {
        Ok(config) => config,
        Err(message) => {
            return RsiServeExit::Failure {
                message,
                exit_code: 2,
            }
        }
    };
    let llm = match wire_rsi_llm(&config) {
        Ok(llm) => llm,
        Err(message) => {
            return RsiServeExit::Failure {
                message,
                exit_code: 2,
            }
        }
    };
    let registry = match wire_bootstrapped_component_registry(&config) {
        Ok(registry) => registry,
        Err(error) => {
            return RsiServeExit::Failure {
                message: format!("装配组件 registry 失败：{}", error.message()),
                exit_code: 2,
            }
        }
    };
    let lesson_store = wire_lesson_store();
    let mut tasks = TaskRegistry::new();
    if let Err(error) = tasks.register(
        harness::domain::task::TaskKind::HarnessRsi,
        Box::new(harness::domain::cycle_task::HarnessRsiCycleTask::new(
            registry,
            llm,
            lesson_store,
            config.harness.rsi.clone(),
        )),
    ) {
        return RsiServeExit::Failure {
            message: format!("注册 HarnessRsi 任务失败：{}", error.message()),
            exit_code: 2,
        };
    }

    let interval = std::time::Duration::from_secs(config.harness.rsi.interval_secs.max(1));
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let ctrl_c = tokio::signal::ctrl_c();

    on_ready(&sock);
    let daemon = run_rsi_daemon(endpoints, tasks, interval, shutdown_rx);
    tokio::select! {
        result = daemon => match result {
            Ok(()) => RsiServeExit::Stopped { sock },
            Err(error) => RsiServeExit::Failure {
                message: format!("rsi serve 运行失败：{error}"),
                exit_code: 2,
            },
        },
        _ = ctrl_c => {
            let _ = shutdown_tx.send(());
            RsiServeExit::Stopped { sock }
        },
    }
}

// ── 生产 LLM 装配与执行模式（#2017）──

use async_trait::async_trait;
use harness::domain::errors::HarnessError;
use harness::domain::improvement::{RsiLlmCompletion, RsiLlmPort, RsiLlmUsage};
use harness::domain::lesson::LessonStore;
use provider::composition::{LlmClient, ProviderClientSpecData};
use provider::published_language::{ModelInfo, ProviderContentData, ProviderRequestData};
use share::reasoning::ReasoningLevel;
use std::collections::HashMap;
use std::sync::Arc;

/// RSI 执行的退出结果（run 模式）。
#[derive(Debug)]
pub enum RsiRunExit {
    Done { summary: String },
    Failure { message: String, exit_code: i32 },
}

/// 生产 LLM 端口：经 provider 单轮补全（invoke 聚合流）。
struct ProductionRsiLlm {
    client: LlmClient,
    model: ModelInfo,
}

impl ProductionRsiLlm {
    async fn invoke_text(&self, prompt: &str) -> Result<RsiLlmCompletion, HarnessError> {
        // 结构化 JSON 输出任务显式低思考档（对齐反思链路教训：高思考
        // 档会把输出预算耗在 thinking 上导致正文为空）——取模型阶梯
        // 最低档；空阶梯 fallback Off。
        let reasoning = self
            .model
            .supported_reasoning
            .first()
            .copied()
            .unwrap_or(ReasoningLevel::Off);
        let request = ProviderRequestData::new(
            self.model.model.clone(),
            [share::message::Message::user(prompt.to_string())],
            16384,
            reasoning,
        );
        let response = self
            .client
            .invoke(&self.model, &request)
            .await
            .map_err(|error| {
                HarnessError::unavailable("harness", format!("RSI 模型调用失败：{error}"))
            })?;
        let text = response
            .output
            .iter()
            .filter_map(|block| match block {
                ProviderContentData::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>();
        let usage = response
            .token_usage
            .map(|usage| RsiLlmUsage {
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                cache_read_tokens: usage.cache_read_tokens,
                cache_write_tokens: usage.cache_write_tokens,
                reasoning_tokens: usage.reasoning_tokens,
            })
            .unwrap_or_default();
        // 空文本在这里保持 Ok，由 RsiLlmPort::complete_with_empty_retry 统一
        // 重试一次；连续空文本才折叠为 Unavailable 错误。
        Ok(RsiLlmCompletion {
            text,
            usage,
            attempts: 1,
        })
    }
}

#[async_trait]
impl RsiLlmPort for ProductionRsiLlm {
    async fn complete(&self, prompt: &str) -> Result<String, HarnessError> {
        Ok(self.complete_with_empty_retry(prompt).await?.text)
    }

    async fn complete_with_usage(&self, prompt: &str) -> Result<RsiLlmCompletion, HarnessError> {
        self.invoke_text(prompt).await
    }
}

/// 装配生产 RSI LLM 端口：default binding 解析 → provider 客户端。
pub fn wire_rsi_llm(config: &share::config::Config) -> Result<Arc<dyn RsiLlmPort>, String> {
    // harness.rsi.model 独立档优先；空则继承 models.default（对齐
    // memory.reflection.model 范式——改进模型可与对话模型分离）。
    let resolved = if config.harness.rsi.model.trim().is_empty() {
        config
            .models
            .resolve_default_model()
            .map_err(|error| format!("解析默认模型失败：{error}"))?
    } else {
        config
            .models
            .resolve_model_selection(&config.harness.rsi.model)
            .map_err(|error| format!("解析 harness.rsi.model 失败：{error}"))?
    };
    let spec = ProviderClientSpecData {
        driver: resolved.driver.clone(),
        source_key: resolved.source_key.clone(),
        // 模型级 api_style 与主链同源（openai driver 默认 Responses API，
        // 兼容网关模型需 chat_completions——None 会 404）。
        api_style: resolved.model.api_style.clone(),
        api_key: resolved.source_config.api_key.clone(),
        base_url: if resolved.source_config.base_url.is_empty() {
            None
        } else {
            Some(resolved.source_config.base_url.clone())
        },
        model: resolved.model.id.clone(),
        max_tokens: resolved.model.max_tokens,
        // #2032 阶梯装配：requested=false 会被连续化档位判为 effective 超
        // 限；声明支持并按模型阶梯取最低档请求（结构化输出低思考）。
        reasoning: true,
        reasoning_config: None,
        timeout_secs: 300,
        // UA 走 snapshot 装配（Provider专属→全局→catalog→内置，#1851 链），
        // 与主会话同源——裸 config.api.user_agent 会被网关按 UA 路由拒绝
        // （token-plan 实证 404 model unavailable）。
        user_agent: Some(
            share::config::domain::snapshot::ConfigSnapshot::new(config.clone())
                .user_agent()
                .to_string(),
        ),
    };
    let client = LlmClient::from_config(spec)
        .map_err(|error| format!("构造 provider 客户端失败：{error}"))?;
    let model = ModelInfo {
        provider: resolved.source_key.clone(),
        model: resolved.model.id.clone(),
        supports_tools: false,
        supports_parallel_tool_calls: false,
        supports_streaming: true,
        supported_reasoning: vec![ReasoningLevel::Off],
        server_effort_map: Vec::new(),
        context_limit: Some(128 * 1024),
        output_limit: Some(4096),
    };
    Ok(Arc::new(ProductionRsiLlm { client, model }))
}

#[cfg(test)]
#[path = "harness_tests.rs"]
mod harness_tests;

/// 内建 prose 组件的装配模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComponentBootstrapMode {
    /// 历史查询/回滚等只读命令：不创建、不虚拟缺失组件。
    None,
    /// `rsi run --dry-run`：缺失内建职责面只在 registry 中虚拟可见。
    DryRun,
    /// 正式 RSI 运行/daemon：缺失内建职责面落盘最小骨架。
    Formal,
}

#[derive(serde::Deserialize)]
struct ProjectSessionSnapshot {
    workspace: ProjectWorkspaceSnapshot,
}

#[derive(serde::Deserialize)]
struct ProjectWorkspaceSnapshot {
    state: String,
    value: ProjectWorkspaceValue,
}

#[derive(serde::Deserialize)]
struct ProjectWorkspaceValue {
    project_identity: share::session_types::ProjectIdentityData,
}

/// 从 `<agents>/session/<project-segment>/` 反解项目 overlay 落点。
///
/// 项目段本身是单向哈希，NEVER 从路径猜测项目根；权威来源是同目录 canonical
/// session 的 workspace 快照。每个项目根按运行时注入优先级选择：
/// `AGENTS.md` 存在则优先，否则 `CLAUDE.md`；两者都缺失时不注册项目组件，
/// NEVER 自动创建项目说明文件。
fn project_overlay_component_paths_under(
    agents_dir: &Path,
) -> HashMap<harness::ComponentId, std::path::PathBuf> {
    let session_root = agents_dir.join("session");
    let mut paths = HashMap::new();
    let Ok(project_entries) = std::fs::read_dir(&session_root) else {
        return paths;
    };
    for project_entry in project_entries.flatten() {
        let project_dir = project_entry.path();
        let Some(segment) = project_dir
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        if !project_dir.is_dir() || !is_project_layout_segment(segment.as_str()) {
            continue;
        }
        let Some(root) = resolve_project_root_from_snapshots(&project_dir) else {
            continue;
        };
        let agents_md = root.join("AGENTS.md");
        let claude_md = root.join("CLAUDE.md");
        let target = if agents_md.is_file() {
            agents_md
        } else if claude_md.is_file() {
            claude_md
        } else {
            continue;
        };
        paths.insert(
            harness::ComponentId {
                kind: harness::ComponentKind::SystemPromptOverlay,
                name: harness::ComponentName::new(segment),
            },
            target,
        );
    }
    paths
}

fn resolve_project_root_from_snapshots(project_dir: &Path) -> Option<std::path::PathBuf> {
    let mut candidates: Vec<(u64, PathBuf)> = std::fs::read_dir(project_dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path.extension().is_none()
                && !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with('.'))
        })
        .filter_map(|path| {
            let size = path.metadata().ok()?.len();
            Some((size, path))
        })
        .collect();
    candidates.sort_by_key(|(size, _)| *size);
    for (_, path) in candidates {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(snapshot) = serde_json::from_str::<ProjectSessionSnapshot>(&text) else {
            continue;
        };
        if snapshot.workspace.state != "captured" {
            continue;
        }
        let identity = snapshot.workspace.value.project_identity;
        let root = match identity.git_common_dir.as_deref() {
            Some(common_dir) => Path::new(common_dir)
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| Path::new(common_dir).to_path_buf()),
            None => Path::new(&identity.initial_cwd).to_path_buf(),
        };
        if root.is_dir() {
            return Some(root);
        }
    }
    None
}

fn builtin_prose_component_paths_under(
    agents_dir: &Path,
) -> HashMap<harness::ComponentId, std::path::PathBuf> {
    let mut component_paths = HashMap::new();
    component_paths.insert(
        harness::ComponentId {
            kind: harness::ComponentKind::Guardrails,
            name: harness::ComponentName::new("default"),
        },
        agents_dir.join("guardrails.md"),
    );
    component_paths.insert(
        harness::ComponentId {
            kind: harness::ComponentKind::ToolGuidance,
            name: harness::ComponentName::new("default"),
        },
        agents_dir.join("tool_guidance.md"),
    );
    component_paths.insert(
        harness::ComponentId {
            kind: harness::ComponentKind::SystemPromptOverlay,
            name: harness::ComponentName::new("global"),
        },
        agents_dir.join("AGENTS.md"),
    );
    component_paths
}

fn builtin_prose_bootstrap_defaults() -> HashMap<harness::ComponentId, harness::ComponentContent> {
    let mut defaults = HashMap::new();
    defaults.insert(
        harness::ComponentId {
            kind: harness::ComponentKind::Guardrails,
            name: harness::ComponentName::new("default"),
        },
        harness::ComponentContent {
            frontmatter: None,
            body: "# Guardrails\n".to_string(),
        },
    );
    defaults.insert(
        harness::ComponentId {
            kind: harness::ComponentKind::ToolGuidance,
            name: harness::ComponentName::new("default"),
        },
        harness::ComponentContent {
            frontmatter: None,
            body: "# Tool Guidance\n".to_string(),
        },
    );
    defaults
}

/// 正式装配前补齐内建 prose 职责面：只创建缺失骨架，NEVER 覆盖用户文件。
fn ensure_builtin_prose_components_at(
    agents_dir: &Path,
) -> Result<Vec<harness::ComponentId>, HarnessError> {
    let component_paths = builtin_prose_component_paths_under(agents_dir);
    let defaults = builtin_prose_bootstrap_defaults();
    let mut ids: Vec<_> = defaults.keys().cloned().collect();
    ids.sort();
    let mut missing = Vec::new();
    for id in ids {
        let Some(content) = defaults.get(&id) else {
            continue;
        };
        let Some(path) = component_paths.get(&id) else {
            continue;
        };
        if !path.exists() {
            missing.push((id, content.clone()));
        }
    }
    let mut registry =
        harness::adapters::filesystem::FilesystemComponentRegistry::new(component_paths);
    let mut created = Vec::new();
    for (id, content) in missing {
        registry.write(&id, &content)?;
        log::info!(target: crate::LOG_TARGET, "内建 prose 组件缺失，已创建最小骨架：{id}");
        created.push(id);
    }
    Ok(created)
}

fn wire_component_registry_under(
    agents_dir: &Path,
    mode: ComponentBootstrapMode,
) -> Result<Arc<tokio::sync::Mutex<Box<dyn harness::ComponentRegistry + Send>>>, HarnessError> {
    if mode == ComponentBootstrapMode::Formal {
        ensure_builtin_prose_components_at(agents_dir)?;
    }
    let mut component_paths = builtin_prose_component_paths_under(agents_dir);
    component_paths.extend(project_overlay_component_paths_under(agents_dir));
    let prose_registry: Box<dyn harness::ComponentRegistry + Send> = match mode {
        ComponentBootstrapMode::DryRun => Box::new(
            harness::adapters::filesystem::FilesystemComponentRegistry::with_bootstrap_defaults(
                component_paths,
                builtin_prose_bootstrap_defaults(),
            ),
        ),
        ComponentBootstrapMode::None | ComponentBootstrapMode::Formal => Box::new(
            harness::adapters::filesystem::FilesystemComponentRegistry::new(component_paths),
        ),
    };
    let skills_registry: Box<dyn harness::ComponentRegistry + Send> = Box::new(
        harness::adapters::skills_registry::SkillsDirectoryRegistry::new(agents_dir.join("skills")),
    );
    // c8 plugins 不进 RSI 可编辑面（2026-10-24 拍板：客户端/UI 相关，
    // 暂停 RSI 自动编辑；seam 执行面（run 边界 reload）保持不变）。
    let composite = harness::adapters::composite::CompositeComponentRegistry::new(vec![
        prose_registry,
        skills_registry,
    ]);
    Ok(Arc::new(tokio::sync::Mutex::new(Box::new(composite))))
}

/// 装配组件 registry：全局 guardrails/tool_guidance（FS）+ scripts 目录
/// 扫描，经组合 registry 合并为单一视图。组件面按接入进度扩展
///（skills 目录扫描随 #2018）。
pub fn wire_component_registry(
    _config: &share::config::Config,
) -> Result<Arc<tokio::sync::Mutex<Box<dyn harness::ComponentRegistry + Send>>>, HarnessError> {
    wire_component_registry_under(
        &share::config::paths::global_agents_dir(),
        ComponentBootstrapMode::None,
    )
}

/// 正式 RSI 运行/daemon 装配：缺失的内建 prose 职责面先落盘最小骨架。
pub fn wire_bootstrapped_component_registry(
    _config: &share::config::Config,
) -> Result<Arc<tokio::sync::Mutex<Box<dyn harness::ComponentRegistry + Send>>>, HarnessError> {
    wire_component_registry_under(
        &share::config::paths::global_agents_dir(),
        ComponentBootstrapMode::Formal,
    )
}

/// dry-run 装配：缺失的内建职责面只虚拟可见，NEVER 写盘。
pub fn wire_dry_run_component_registry(
    _config: &share::config::Config,
) -> Result<Arc<tokio::sync::Mutex<Box<dyn harness::ComponentRegistry + Send>>>, HarnessError> {
    wire_component_registry_under(
        &share::config::paths::global_agents_dir(),
        ComponentBootstrapMode::DryRun,
    )
}

/// 经验库路径（`{agents_dir}/harness/library.jsonl`，独立于 memory corpus）。
pub fn wire_lesson_store() -> Arc<LessonStore> {
    Arc::new(LessonStore::new(
        share::config::paths::global_agents_dir().join("harness/library.jsonl"),
    ))
}

/// `aemeath rsi run`：一次性执行单轮改进周期后退出。
/// rsi run 参数覆盖（#2050，仅本次生效）。
pub struct RsiRunOverrides {
    pub max_runs: Option<usize>,
    pub window_days: Option<u64>,
}

pub async fn run_rsi_run_once() -> RsiRunExit {
    run_rsi_run_inner(
        &RsiRunOverrides {
            max_runs: None,
            window_days: None,
        },
        false,
    )
    .await
}

pub async fn run_rsi_run_once_with(overrides: &RsiRunOverrides) -> RsiRunExit {
    run_rsi_run_inner(overrides, false).await
}

/// dry-run：完整周期但零持久副作用（#2050）。
pub async fn run_rsi_run_dry(overrides: &RsiRunOverrides) -> RsiRunExit {
    run_rsi_run_inner(overrides, true).await
}

async fn run_rsi_run_inner(overrides: &RsiRunOverrides, dry_run: bool) -> RsiRunExit {
    // rsi run 是独立 CLI 进程：先装日志（失败不阻断，降级 eprintln）。
    if let Ok(log_config) = load_global_config_layered() {
        let snapshot = share::config::domain::snapshot::ConfigSnapshot::new(log_config);
        let logs_dir = share::config::paths::global_logs_dir();
        if let Err(error) = crate::app::init_logging(
            &snapshot,
            sdk::LoggingOutputMode::File,
            sdk::NativeStderrMode::Preserve,
            &logs_dir,
        ) {
            // 日志初始化失败时 logger 未装——降级无输出，仅放弃观测性
            let _ = error;
        }
    }
    let config = match load_global_config_layered() {
        Ok(config) => config,
        Err(message) => {
            return RsiRunExit::Failure {
                message,
                exit_code: 2,
            }
        }
    };
    let llm = match wire_rsi_llm(&config) {
        Ok(llm) => llm,
        Err(message) => {
            return RsiRunExit::Failure {
                message,
                exit_code: 2,
            }
        }
    };
    let registry_result = if dry_run {
        wire_dry_run_component_registry(&config)
    } else {
        wire_bootstrapped_component_registry(&config)
    };
    let registry = match registry_result {
        Ok(registry) => registry,
        Err(error) => {
            return RsiRunExit::Failure {
                message: format!("装配组件 registry 失败：{}", error.message()),
                exit_code: 2,
            }
        }
    };
    let mut rsi_config = config.harness.rsi.clone();
    if let Some(max_runs) = overrides.max_runs {
        rsi_config.evidence_max_runs_per_cycle = max_runs;
    }
    if let Some(window_days) = overrides.window_days {
        rsi_config.evidence_initial_window_days = window_days;
    }
    let lesson_store = wire_lesson_store();
    let stream = ProductionEvidenceStream {
        datasets_root: share::config::paths::global_session_datasets_dir(),
        harness_dir: share::config::paths::global_agents_dir().join("harness"),
    };
    let task = harness::domain::cycle_task::HarnessRsiCycleTask::new(
        registry,
        llm,
        lesson_store,
        rsi_config,
    )
    .with_evidence_stream(Box::new(stream))
    // #2070：过程事件记录器（fail-open；GC 由 recorder 自管）
    .with_event_recorder(std::sync::Arc::new(
        harness::domain::events::RsiEventRecorder::new(
            share::config::paths::global_agents_dir().join("harness/events"),
        ),
    ))
    .with_version_store(std::sync::Arc::new(
        harness::domain::versioning::ComponentVersionStore::new(
            share::config::paths::global_agents_dir().join("harness/versions"),
        ),
    ))
    .with_history_path(
        share::config::paths::global_agents_dir().join("harness/signal-history.json"),
    );
    let mut task = task;
    if dry_run {
        task = task.with_dry_run();
    }
    use harness::domain::task::RsiTask;
    match task.run_cycle().await {
        Ok(outcome) => {
            if dry_run {
                write_dry_run_preview(&outcome.summary);
            }
            RsiRunExit::Done {
                summary: outcome.summary,
            }
        }
        Err(error) => RsiRunExit::Failure {
            message: format!("改进周期失败：{}", error.message()),
            exit_code: 1,
        },
    }
}

/// 生产证据流端口（#2049 流模型）：plan（游标+窗口+步长+分块）→
/// block_evidence（扫描聚合）→ advance（游标推进 + 指纹基线）。
struct ProductionEvidenceStream {
    datasets_root: std::path::PathBuf,
    harness_dir: std::path::PathBuf,
}

impl harness::EvidenceStreamPort for ProductionEvidenceStream {
    fn plan(&self, params: &harness::StreamPlanParams) -> Vec<harness::EvidenceBlockPlan> {
        let entries = enumerate_session_entries(&self.datasets_root);
        let state = match harness::load_stream_state(&self.harness_dir) {
            Ok(state) => state,
            Err(error) => {
                log::warn!(target: crate::LOG_TARGET, "checkpoint 读取失败，按首次处理：{}", error.message());
                harness::EvidenceStreamState::default()
            }
        };
        // session → run 清单（轻量 cursor 提取）
        let mut session_runs = std::collections::BTreeMap::new();
        for entry in &entries {
            let run_ids = harness::adapters::session_scanner::dataset_run_ids(
                &self.datasets_root.join(&entry.session_key),
            );
            session_runs.insert(entry.session_key.clone(), run_ids);
        }
        harness::plan_evidence_stream(entries, &state, &session_runs, params)
    }

    fn block_evidence(&self, block: &harness::EvidenceBlockPlan) -> harness::EvidenceBundle {
        let mut events = Vec::new();
        // 按 session 分组扫描，块内过滤 run
        let mut by_session: std::collections::BTreeMap<&str, std::collections::BTreeSet<&str>> =
            Default::default();
        for run_ref in &block.runs {
            by_session
                .entry(run_ref.session_key.as_str())
                .or_default()
                .insert(run_ref.run_id.as_str());
        }
        for (session_key, run_ids) in by_session {
            let report =
                harness::adapters::session_scanner::SessionDatasetScanner::scan_dataset_under_root(
                    &self.datasets_root,
                    session_key,
                );
            events.extend(
                report
                    .events
                    .into_iter()
                    .filter(|event| run_ids.contains(event.run_id.as_str())),
            );
        }
        harness::aggregate_evidence(&events)
    }

    fn read_context(
        &self,
        source: &harness::domain::evidence::EvidenceSourceRef,
    ) -> Option<harness::domain::evidence::SessionContextExcerpt> {
        harness::adapters::session_scanner::SessionDatasetScanner::read_step_context_under_root(
            &self.datasets_root,
            source,
        )
    }

    fn cursor_summary(&self) -> Option<String> {
        harness::load_stream_state(&self.harness_dir)
            .ok()
            .and_then(|state| {
                state
                    .sessions
                    .iter()
                    .rfind(|(_, progress)| {
                        !progress.session_complete || !progress.processed_run_ids.is_empty()
                    })
                    .map(|(key, progress)| {
                        format!(
                            "{}（runs {}，complete={}）",
                            key,
                            progress.processed_run_ids.len(),
                            progress.session_complete
                        )
                    })
            })
    }

    fn advance(&self, block: &harness::EvidenceBlockPlan, _fingerprint: String) {
        let mut state = harness::load_stream_state(&self.harness_dir).unwrap_or_default();
        let mut touched_sessions: Vec<String> = Vec::new();
        for run_ref in &block.runs {
            let progress = state
                .sessions
                .entry(run_ref.session_key.clone())
                .or_default();
            if !progress.processed_run_ids.contains(&run_ref.run_id) {
                progress.processed_run_ids.push(run_ref.run_id.clone());
            }
            if !touched_sessions.contains(&run_ref.session_key) {
                touched_sessions.push(run_ref.session_key.clone());
            }
        }
        // session_complete：该 session 盘上全部 run 均已处理
        for session_key in touched_sessions {
            let all_runs = harness::adapters::session_scanner::dataset_run_ids(
                &self.datasets_root.join(&session_key),
            );
            if let Some(progress) = state.sessions.get_mut(&session_key) {
                progress.session_complete = all_runs
                    .iter()
                    .all(|run_id| progress.processed_run_ids.contains(run_id));
            }
        }
        let live: std::collections::BTreeSet<String> =
            enumerate_session_entries(&self.datasets_root)
                .into_iter()
                .map(|entry| entry.session_key)
                .collect();
        if let Err(error) = harness::save_stream_state(&self.harness_dir, &state, &live) {
            log::warn!(target: crate::LOG_TARGET, "checkpoint 推进失败（下周期将重读本块）：{}", error.message());
        }
    }
}

/// 枚举 session dataset 根的流条目（mtime 全序原料）。
/// 只支持项目布局 `<project>/<session>.dataset`；顶层 legacy
/// `<session>.dataset` 是旧设计，RSI NEVER 枚举。项目段保留在 session key
/// 中，供 RSI 区分项目/全局落点。
fn enumerate_session_entries(datasets_root: &std::path::Path) -> Vec<harness::SessionStreamEntry> {
    let mut entries = Vec::new();
    let Ok(project_dirs) = std::fs::read_dir(datasets_root) else {
        return entries;
    };
    for project_entry in project_dirs.flatten() {
        let project_path = project_entry.path();
        let Some(project_segment) = project_path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        if !project_path.is_dir() || !is_project_layout_segment(&project_segment) {
            continue;
        }
        let Ok(dataset_entries) = std::fs::read_dir(&project_path) else {
            continue;
        };
        for dataset_entry in dataset_entries.flatten() {
            let dataset_path = dataset_entry.path();
            if dataset_path.is_dir()
                && dataset_path.extension().and_then(|ext| ext.to_str()) == Some("dataset")
            {
                push_session_entry(&mut entries, &dataset_path, Some(&project_segment));
            }
        }
    }
    entries
}

/// session 存储的项目段：16 位小写 hex（project identity 稳定哈希）。
fn is_project_layout_segment(value: &str) -> bool {
    value.len() == 16
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn push_session_entry(
    entries: &mut Vec<harness::SessionStreamEntry>,
    dataset_path: &std::path::Path,
    project_segment: Option<&str>,
) {
    let Some(session_name) = dataset_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
    else {
        return;
    };
    let session_key = match project_segment {
        Some(project) => format!("{project}/{session_name}"),
        None => session_name,
    };
    let updated_at_secs = dataset_path
        .metadata()
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    entries.push(harness::SessionStreamEntry {
        session_key,
        updated_at_secs,
    });
}

/// 最小配置加载：默认值 + `~/.agents/aemeath.json` patch 层合并。
/// （rsi run 不复用会话级 config wiring——独立进程面保持自包含。）
fn load_global_config_layered() -> Result<share::config::Config, String> {
    let config_path = share::config::paths::global_config_path();
    let base = share::config::Config::default();
    let text = match std::fs::read_to_string(&config_path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(base),
        Err(error) => return Err(format!("读取配置失败 {}: {error}", config_path.display())),
    };
    let patch: share::config::domain::merge::ConfigPatch = if text.trim().is_empty() {
        return Ok(base);
    } else {
        serde_json::from_str(&text).map_err(|error| format!("解析配置失败：{error}"))?
    };
    let mut chain = share::config::domain::merge::PriorityChain::new();
    chain.push(patch);
    Ok(chain.merge(base))
}

/// dry-run 预览落盘（ALWAYS 覆盖写，NEVER 累积；#2050）。
fn write_dry_run_preview(summary: &str) {
    let harness_dir = share::config::paths::global_agents_dir().join("harness");
    if let Err(error) = std::fs::create_dir_all(&harness_dir) {
        log::warn!(target: crate::LOG_TARGET, "创建 harness 目录失败：{error}");
        return;
    }
    let preview_path = harness_dir.join("dry-run-preview.md");
    if let Err(error) = std::fs::write(&preview_path, summary) {
        log::warn!(target: crate::LOG_TARGET, "写 dry-run 预览失败：{error}");
    }
}

/// `rsi history [--component <kind>/<name>]`（#2074）：列组件快照历史。
pub fn run_rsi_history(component: Option<&str>) -> RsiRunExit {
    let config = match load_global_config_layered() {
        Ok(config) => config,
        Err(message) => {
            return RsiRunExit::Failure {
                message,
                exit_code: 2,
            }
        }
    };
    let registry = match wire_component_registry(&config) {
        Ok(registry) => registry,
        Err(error) => {
            return RsiRunExit::Failure {
                message: format!("装配组件 registry 失败：{}", error.message()),
                exit_code: 2,
            }
        }
    };
    let store = harness::domain::versioning::ComponentVersionStore::new(
        share::config::paths::global_agents_dir().join("harness/versions"),
    );
    let ids: Vec<harness::ComponentId> = match component {
        Some(raw) => match harness::parse_component_id(raw) {
            Some(id) => vec![id],
            None => {
                return RsiRunExit::Failure {
                    message: format!("组件标识无效（需要 <kind>/<name>）：{raw}"),
                    exit_code: 2,
                }
            }
        },
        None => {
            let guard = match registry.try_lock() {
                Ok(guard) => guard,
                Err(_) => {
                    return RsiRunExit::Failure {
                        message: "registry 锁不可用".to_string(),
                        exit_code: 1,
                    }
                }
            };
            guard.list()
        }
    };
    let mut lines = Vec::new();
    for id in &ids {
        match store.history(id) {
            Ok(history) if history.is_empty() => {}
            Ok(history) => {
                for snapshot in history {
                    lines.push(format!(
                        "{}  {}  action={}  {}",
                        id,
                        snapshot.cycle_id,
                        snapshot.action,
                        match &snapshot.payload {
                            harness::domain::versioning::SnapshotContent::Existing { raw } => {
                                format!("原文 {} 字符", raw.chars().count())
                            }
                            harness::domain::versioning::SnapshotContent::Tombstone => {
                                "（新建前）".to_string()
                            }
                        }
                    ));
                }
            }
            Err(error) => {
                return RsiRunExit::Failure {
                    message: format!("读取历史失败：{}", error.message()),
                    exit_code: 1,
                }
            }
        }
    }
    RsiRunExit::Done {
        summary: if lines.is_empty() {
            "（无快照历史）".to_string()
        } else {
            lines.join("\n")
        },
    }
}

/// `rsi rollback <kind>/<name> [--cycle <id>]`（#2074）：恢复组件到快照。
pub fn run_rsi_rollback(component: &str, cycle: Option<&str>) -> RsiRunExit {
    let config = match load_global_config_layered() {
        Ok(config) => config,
        Err(message) => {
            return RsiRunExit::Failure {
                message,
                exit_code: 2,
            }
        }
    };
    let registry = match wire_component_registry(&config) {
        Ok(registry) => registry,
        Err(error) => {
            return RsiRunExit::Failure {
                message: format!("装配组件 registry 失败：{}", error.message()),
                exit_code: 2,
            }
        }
    };
    let Some(id) = harness::parse_component_id(component) else {
        return RsiRunExit::Failure {
            message: format!("组件标识无效（需要 <kind>/<name>）：{component}"),
            exit_code: 2,
        };
    };
    let store = harness::domain::versioning::ComponentVersionStore::new(
        share::config::paths::global_agents_dir().join("harness/versions"),
    );
    let snapshot = match store.load(&id, cycle) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return RsiRunExit::Failure {
                message: format!("加载快照失败：{}", error.message()),
                exit_code: 1,
            }
        }
    };
    let mut guard = match registry.try_lock() {
        Ok(guard) => guard,
        Err(_) => {
            return RsiRunExit::Failure {
                message: "registry 锁不可用".to_string(),
                exit_code: 1,
            }
        }
    };
    match harness::domain::versioning::rollback_component(&mut **guard, &id, &snapshot) {
        Ok(message) => RsiRunExit::Done { summary: message },
        Err(error) => RsiRunExit::Failure {
            message: format!("回滚失败：{}", error.message()),
            exit_code: 1,
        },
    }
}
