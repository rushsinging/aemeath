//! composition 装配：RSI 维护 daemon 的组合根。
//!
//! 任务集（TaskKind 装配）在此唯一装配：本阶段注册空任务集
//! （改进循环实现由后续 issue 接入），daemon 框架面先稳定。

use harness::adapters::daemon_serve::{run_rsi_daemon, RsiDaemonEndpoints};
use harness::domain::task::TaskRegistry;
use std::path::Path;

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
    let registry = match wire_component_registry(&config) {
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
use harness::domain::improvement::RsiLlmPort;
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

#[async_trait]
impl RsiLlmPort for ProductionRsiLlm {
    async fn complete(&self, prompt: &str) -> Result<String, HarnessError> {
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
        if text.trim().is_empty() {
            return Err(HarnessError::unavailable(
                "harness",
                "RSI 模型返回空文本（结构化输出任务勿用高思考档——harness.rsi.* 档位独立配置随信号装配接入）",
            ));
        }
        Ok(text)
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

/// 装配组件 registry：全局 guardrails/tool_guidance（FS）+ scripts 目录
/// 扫描，经组合 registry 合并为单一视图。组件面按接入进度扩展
///（skills 目录扫描随 #2018）。
pub fn wire_component_registry(
    config: &share::config::Config,
) -> Result<Arc<tokio::sync::Mutex<Box<dyn harness::ComponentRegistry + Send>>>, HarnessError> {
    let mut component_paths = HashMap::new();
    component_paths.insert(
        harness::ComponentId {
            kind: harness::ComponentKind::Guardrails,
            name: harness::ComponentName::new("default"),
        },
        share::config::paths::global_guardrails_path(),
    );
    component_paths.insert(
        harness::ComponentId {
            kind: harness::ComponentKind::ToolGuidance,
            name: harness::ComponentName::new("default"),
        },
        share::config::paths::global_tool_guidance_path(),
    );
    // overlay（c1）：全局 AGENTS.md（#2019 registry 化；项目链注入侧不变）
    component_paths.insert(
        harness::ComponentId {
            kind: harness::ComponentKind::SystemPromptOverlay,
            name: harness::ComponentName::new("global"),
        },
        share::config::paths::global_agents_md_path(),
    );
    let prose_registry: Box<dyn harness::ComponentRegistry + Send> =
        Box::new(harness::adapters::filesystem::FilesystemComponentRegistry::new(component_paths));
    let skills_registry: Box<dyn harness::ComponentRegistry + Send> = Box::new(
        harness::adapters::skills_registry::SkillsDirectoryRegistry::new(
            share::config::paths::global_skills_dir(),
        ),
    );
    // c8 plugins 不进 RSI 可编辑面（2026-10-24 拍板：客户端/UI 相关，
    // 暂停 RSI 自动编辑；seam 执行面（run 边界 reload）保持不变）。
    let _ = config;
    let composite = harness::adapters::composite::CompositeComponentRegistry::new(vec![
        prose_registry,
        skills_registry,
    ]);
    Ok(Arc::new(tokio::sync::Mutex::new(Box::new(composite))))
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
    pub max_sessions: Option<usize>,
    pub window_days: Option<u64>,
}

pub async fn run_rsi_run_once() -> RsiRunExit {
    run_rsi_run_inner(
        &RsiRunOverrides {
            max_sessions: None,
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
    let registry = match wire_component_registry(&config) {
        Ok(registry) => registry,
        Err(error) => {
            return RsiRunExit::Failure {
                message: format!("装配组件 registry 失败：{}", error.message()),
                exit_code: 2,
            }
        }
    };
    let mut rsi_config = config.harness.rsi.clone();
    if let Some(max_sessions) = overrides.max_sessions {
        rsi_config.evidence_max_sessions_per_cycle = max_sessions;
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
            let report = harness::adapters::session_scanner::SessionDatasetScanner::scan_dataset(
                &self.datasets_root.join(session_key),
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
        let live: std::collections::BTreeSet<String> = std::fs::read_dir(&self.datasets_root)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|entry| {
                        entry.path().is_dir()
                            && entry
                                .path()
                                .extension()
                                .and_then(|ext| ext.to_str())
                                .is_some_and(|ext| ext == "dataset")
                    })
                    .filter_map(|entry| entry.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        if let Err(error) = harness::save_stream_state(&self.harness_dir, &state, &live) {
            log::warn!(target: crate::LOG_TARGET, "checkpoint 推进失败（下周期将重读本块）：{}", error.message());
        }
    }
}

/// 枚举 session dataset 根的流条目（mtime 全序原料）。
fn enumerate_session_entries(datasets_root: &std::path::Path) -> Vec<harness::SessionStreamEntry> {
    let Ok(entries) = std::fs::read_dir(datasets_root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext == "dataset")
        })
        .map(|path| {
            let session_key = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .to_string();
            let updated_at_secs = path
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs())
                .unwrap_or(0);
            harness::SessionStreamEntry {
                session_key,
                updated_at_secs,
            }
        })
        .collect()
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
