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
        let request = ProviderRequestData::new(
            self.model.model.clone(),
            [share::message::Message::user(prompt.to_string())],
            16384,
            ReasoningLevel::Off,
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
    let resolved = config
        .models
        .resolve_default_model()
        .map_err(|error| format!("解析默认模型失败：{error}"))?;
    let spec = ProviderClientSpecData {
        driver: resolved.driver.clone(),
        source_key: resolved.source_key.clone(),
        api_style: None,
        api_key: resolved.source_config.api_key.clone(),
        base_url: if resolved.source_config.base_url.is_empty() {
            None
        } else {
            Some(resolved.source_config.base_url.clone())
        },
        model: resolved.model.name.clone(),
        max_tokens: resolved.model.max_tokens,
        reasoning: false,
        reasoning_config: None,
        timeout_secs: 300,
        user_agent: Some(config.api.user_agent.clone()),
    };
    let client = LlmClient::from_config(spec)
        .map_err(|error| format!("构造 provider 客户端失败：{error}"))?;
    let model = ModelInfo {
        provider: resolved.source_key.clone(),
        model: resolved.model.name.clone(),
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
    let prose_registry: Box<dyn harness::ComponentRegistry + Send> =
        Box::new(harness::adapters::filesystem::FilesystemComponentRegistry::new(component_paths));
    let scripts_registry: Box<dyn harness::ComponentRegistry + Send> = Box::new(
        harness::adapters::scripts_registry::ScriptsDirectoryRegistry::new(
            share::config::paths::global_scripts_dir(),
        ),
    );
    let _ = config;
    let composite = harness::adapters::composite::CompositeComponentRegistry::new(vec![
        prose_registry,
        scripts_registry,
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
pub async fn run_rsi_run_once() -> RsiRunExit {
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
    let lesson_store = wire_lesson_store();
    let mut task = harness::domain::cycle_task::HarnessRsiCycleTask::new(
        registry,
        llm,
        lesson_store,
        config.harness.rsi.clone(),
    );
    use harness::domain::task::RsiTask;
    match task.run_cycle().await {
        Ok(outcome) => RsiRunExit::Done {
            summary: outcome.summary,
        },
        Err(error) => RsiRunExit::Failure {
            message: format!("改进周期失败：{}", error.message()),
            exit_code: 1,
        },
    }
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
