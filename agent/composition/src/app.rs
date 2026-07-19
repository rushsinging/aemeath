use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use runtime::ProviderFactory;
use sdk::{AgentClient, MemoryConfigView, SdkError, SkillView};
use tools::ToolCatalogGateway;

use crate::runtime::{AgentArgs, AgentClientImpl};
use logging::{LoggingOutputMode, LoggingSettings, UnifiedLogger};
use share::config::domain::snapshot::ConfigSnapshot;
use std::path::Path;

pub type AgentClientHandle = Arc<dyn AgentClient>;

pub struct AgentClientBootstrap {
    pub client: AgentClientHandle,
    pub session_id: String,
    pub cwd: PathBuf,
    pub model_display: String,
    pub allow_all: bool,
    pub context_size: usize,
    pub thinking: bool,
    pub memory_config: MemoryConfigView,
    pub skills_map: HashMap<String, SkillView>,
}

pub fn agent_client_from_runtime(client: AgentClientImpl) -> AgentClientHandle {
    Arc::new(client)
}

pub struct FeatureGateways {
    pub tools: Arc<dyn ToolCatalogGateway>,
    pub provider: Arc<dyn ProviderFactory>,
    pub policy: Arc<dyn policy::PolicyPort>,
}

impl FeatureGateways {
    pub fn new(
        tools: Arc<dyn ToolCatalogGateway>,
        provider: Arc<dyn ProviderFactory>,
        policy: Arc<dyn policy::PolicyPort>,
    ) -> Self {
        Self {
            tools,
            provider,
            policy,
        }
    }

    pub fn wire_default(policy: Arc<dyn policy::PolicyPort>) -> Self {
        Self::new(
            crate::tools::wire_tools(),
            crate::provider::provider_factory(),
            policy,
        )
    }
}

struct ConfigPolicyModeSource {
    reader: Arc<dyn config::ConfigReader>,
}

impl policy::PolicyModeSource for ConfigPolicyModeSource {
    fn current_mode(&self) -> policy::PolicyMode {
        self.reader.committed_snapshot().permission_mode().into()
    }
}

fn configured_policy(config: &config::ConfigWiring) -> Arc<dyn policy::PolicyPort> {
    Arc::new(policy::ConfiguredPolicy::new(ConfigPolicyModeSource {
        reader: config.reader(),
    }))
}

fn cli_config_input(args: &AgentArgs) -> config::CliConfigInput {
    config::CliConfigInput {
        api_key: args.api_key.clone(),
        base_url: args.base_url.clone(),
        model: args.model.clone(),
        max_tokens: args.max_tokens,
        context_size: (args.context_size > 0).then_some(args.context_size),
        allow_all: args.allow_all,
        verbose: args.verbose,
        no_markdown: args.no_markdown,
        max_tool_concurrency: args.max_tool_concurrency,
        max_agent_concurrency: args.max_agent_concurrency,
    }
}

fn logging_settings_from_snapshot(
    snapshot: &ConfigSnapshot,
    default_logs_dir: &Path,
    output_mode: LoggingOutputMode,
) -> LoggingSettings {
    LoggingSettings::new(
        snapshot.logging_level().to_string(),
        output_mode,
        snapshot
            .logs_dir()
            .map(PathBuf::from)
            .unwrap_or_else(|| default_logs_dir.to_path_buf()),
        snapshot.logging_max_bytes(),
        snapshot.logging_max_backups(),
        snapshot.logging_retention_days(),
    )
}

fn logging_settings_from_bootstrap(
    snapshot: &ConfigSnapshot,
    default_logs_dir: &Path,
    output_mode: sdk::LoggingOutputMode,
) -> LoggingSettings {
    let output_mode = match output_mode {
        sdk::LoggingOutputMode::File => LoggingOutputMode::File,
        sdk::LoggingOutputMode::Stderr => LoggingOutputMode::Stderr,
    };
    logging_settings_from_snapshot(snapshot, default_logs_dir, output_mode)
}

static LOGGING_INIT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoggingInitDecision {
    Initialize,
    AlreadyInitialized,
}

fn logging_init_decision(
    current: Option<LoggingOutputMode>,
    requested: LoggingOutputMode,
) -> Result<LoggingInitDecision, String> {
    match current {
        None => Ok(LoggingInitDecision::Initialize),
        Some(existing) if existing == requested => Ok(LoggingInitDecision::AlreadyInitialized),
        Some(existing) => Err(format!(
            "logging already initialized with output mode {existing:?}; requested {requested:?} conflicts"
        )),
    }
}

fn init_logging(
    snapshot: &ConfigSnapshot,
    output_mode: sdk::LoggingOutputMode,
) -> Result<(), String> {
    let _guard = LOGGING_INIT_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "日志初始化锁已损坏".to_string())?;
    let requested_output_mode = match output_mode {
        sdk::LoggingOutputMode::File => LoggingOutputMode::File,
        sdk::LoggingOutputMode::Stderr => LoggingOutputMode::Stderr,
    };
    let current_output_mode = UnifiedLogger::current().map(UnifiedLogger::output_mode);
    match logging_init_decision(current_output_mode, requested_output_mode)? {
        LoggingInitDecision::AlreadyInitialized => return Ok(()),
        LoggingInitDecision::Initialize => {}
    }
    let settings = logging_settings_from_bootstrap(
        snapshot,
        &share::config::paths::global_logs_dir(),
        output_mode,
    );
    UnifiedLogger::init(settings.clone()).map_err(|error| error.to_string())?;
    logging::set_boot_ts(logging::timestamp_local_rfc3339());
    logging::set_app_version(share::version().to_string());
    log::info!(
        target: crate::LOG_TARGET,
        "logging initialized: filter={} mode={:?} logs_dir={} retention_policy_days={}",
        settings.filter_directive(),
        settings.output_mode(),
        settings.logs_dir().display(),
        settings.retention_days(),
    );
    Ok(())
}

pub async fn build_agent_client(args: AgentArgs) -> Result<AgentClientHandle, SdkError> {
    let cwd = args
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let workspace = project::wire_production_workspace(cwd.clone())
        .map_err(|error| SdkError::Init(error.to_string()))?
        .into_views();
    let logging_output = args.logging_output;
    let config = config::wire_project_config_with_cli(&cwd, cli_config_input(&args))
        .await
        .map_err(|error| SdkError::Init(format!("配置初始化失败：{error:?}")))?;
    let gateways = FeatureGateways::wire_default(configured_policy(&config));
    init_logging(&config.reader().committed_snapshot(), logging_output)
        .map_err(|error| SdkError::Init(format!("日志初始化失败：{error}")))?;
    let runtime_client =
        crate::runtime::from_args_with_gateways(args, gateways, workspace, config).await?;
    Ok(agent_client_from_runtime(runtime_client))
}

#[cfg(test)]
async fn build_agent_client_with_gateways(
    args: AgentArgs,
    gateways: FeatureGateways,
) -> Result<AgentClientHandle, SdkError> {
    let cwd = args
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let workspace = project::wire_production_workspace(cwd.clone())
        .map_err(|error| SdkError::Init(error.to_string()))?
        .into_views();
    let logging_output = args.logging_output;
    let config = config::wire_project_config_with_cli(&cwd, cli_config_input(&args))
        .await
        .map_err(|error| SdkError::Init(format!("配置初始化失败：{error:?}")))?;
    init_logging(&config.reader().committed_snapshot(), logging_output)
        .map_err(|error| SdkError::Init(format!("日志初始化失败：{error}")))?;
    let runtime_client =
        crate::runtime::from_args_with_gateways(args, gateways, workspace, config).await?;
    Ok(agent_client_from_runtime(runtime_client))
}

pub async fn build_agent_bootstrap(args: AgentArgs) -> Result<AgentClientBootstrap, SdkError> {
    let cwd = args
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let workspace = project::wire_production_workspace(cwd.clone())
        .map_err(|error| SdkError::Init(error.to_string()))?
        .into_views();
    let logging_output = args.logging_output;
    let config = config::wire_project_config_with_cli(&cwd, cli_config_input(&args))
        .await
        .map_err(|error| SdkError::Init(format!("配置初始化失败：{error:?}")))?;
    let gateways = FeatureGateways::wire_default(configured_policy(&config));
    init_logging(&config.reader().committed_snapshot(), logging_output)
        .map_err(|error| SdkError::Init(format!("日志初始化失败：{error}")))?;
    let runtime_client =
        crate::runtime::from_args_with_gateways(args, gateways, workspace, config).await?;
    let launch = runtime_client.tui_launch_context();
    let thinking = launch.binding.requested_reasoning != provider::ReasoningLevel::Off;
    let client = agent_client_from_runtime(runtime_client);
    let cwd = launch.workspace_root.clone();

    Ok(AgentClientBootstrap {
        client,
        session_id: launch.session_id,
        cwd,
        model_display: launch.model_display,
        allow_all: launch.allow_all,
        context_size: launch.context_size,
        thinking,
        memory_config: launch.memory_config,
        skills_map: launch.skills_map,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use provider::ProviderError;
    use runtime::{ProviderBinding, ProviderBuildSpec, ProviderFactory};
    use share::config::Config;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tools::composition::CountingToolCatalogGateway;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct EnvGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        previous_agents_dir: Option<std::ffi::OsString>,
        previous_home: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(agents_dir: &std::path::Path, home: &std::path::Path) -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
            let previous_agents_dir = std::env::var_os("AEMEATH_AGENTS_DIR");
            let previous_home = std::env::var_os("HOME");
            unsafe {
                std::env::set_var("AEMEATH_AGENTS_DIR", agents_dir);
                std::env::set_var("HOME", home);
            }
            Self {
                _lock: lock,
                previous_agents_dir,
                previous_home,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match self.previous_agents_dir.take() {
                    Some(value) => std::env::set_var("AEMEATH_AGENTS_DIR", value),
                    None => std::env::remove_var("AEMEATH_AGENTS_DIR"),
                }
                match self.previous_home.take() {
                    Some(value) => std::env::set_var("HOME", value),
                    None => std::env::remove_var("HOME"),
                }
            }
        }
    }

    #[test]
    fn logging_init_decision_initializes_when_no_logger_exists() {
        assert_eq!(
            logging_init_decision(None, LoggingOutputMode::File).unwrap(),
            LoggingInitDecision::Initialize
        );
    }

    #[test]
    fn logging_init_decision_is_idempotent_for_same_output_mode() {
        assert_eq!(
            logging_init_decision(Some(LoggingOutputMode::Stderr), LoggingOutputMode::Stderr)
                .unwrap(),
            LoggingInitDecision::AlreadyInitialized
        );
    }

    #[test]
    fn logging_init_decision_rejects_conflicting_output_mode() {
        let error = logging_init_decision(Some(LoggingOutputMode::File), LoggingOutputMode::Stderr)
            .unwrap_err();
        assert!(error.contains("already initialized"));
        assert!(error.contains("File"));
        assert!(error.contains("Stderr"));
    }

    #[derive(Default)]
    struct CountingProviderFactory {
        build_calls: AtomicUsize,
    }

    impl ProviderFactory for CountingProviderFactory {
        fn build(&self, spec: ProviderBuildSpec) -> Result<ProviderBinding, ProviderError> {
            self.build_calls.fetch_add(1, Ordering::SeqCst);
            crate::provider::provider_factory().build(spec)
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn build_agent_client_with_gateways_consumes_injected_provider_and_tools() {
        let temp = tempfile::tempdir().expect("create temp root");
        let root = temp.path().join("root");
        let agents_dir = temp.path().join("agents");
        std::fs::create_dir_all(&root).expect("create project root");
        std::fs::create_dir_all(&agents_dir).expect("create agents dir");
        std::fs::write(
            agents_dir.join("aemeath.json"),
            serde_json::json!({
                "models": {
                    "default": "local/test-model",
                    "providers": {
                        "local": {
                            "baseUrl": "http://127.0.0.1:1/v1",
                            "apiKey": "test-api-key",
                            "driver": "openai",
                            "models": [{
                                "id": "test-model",
                                "name": "Test Model",
                                "input": ["text"],
                                "contextWindow": 8192,
                                "max_tokens": 1024
                            }]
                        }
                    }
                }
            })
            .to_string(),
        )
        .expect("write config");
        std::fs::write(agents_dir.join("mcp.json"), r#"{"mcpServers":{}}"#)
            .expect("write MCP config");

        let _env = EnvGuard::set(&agents_dir, temp.path());

        let provider = Arc::new(CountingProviderFactory::default());
        let tools = Arc::new(CountingToolCatalogGateway::default());
        let gateways = FeatureGateways::new(
            tools.clone(),
            provider.clone(),
            Arc::new(policy::AllowAllPolicy),
        );
        let args = AgentArgs {
            cwd: Some(root),
            api_key: Some("test-api-key".to_string()),
            base_url: Some("http://127.0.0.1:1/v1".to_string()),
            model: Some("local/test-model".to_string()),
            context_size: 8192,
            ..Default::default()
        };

        let result = build_agent_client_with_gateways(args, gateways).await;

        result.expect("build client with injected gateways");
        assert_eq!(provider.build_calls.load(Ordering::SeqCst), 1);
        assert_eq!(tools.new_registry_calls(), 1);
        assert_eq!(tools.register_all_tools_calls(), 1);
    }

    #[test]
    fn typed_bootstrap_output_mode_constructs_logging_settings() {
        let snapshot = ConfigSnapshot::new(Config::default());

        let file = logging_settings_from_bootstrap(
            &snapshot,
            Path::new("/fallback/logs"),
            sdk::LoggingOutputMode::File,
        );
        let stderr = logging_settings_from_bootstrap(
            &snapshot,
            Path::new("/fallback/logs"),
            sdk::LoggingOutputMode::Stderr,
        );

        assert_eq!(file.output_mode(), LoggingOutputMode::File);
        assert_eq!(stderr.output_mode(), LoggingOutputMode::Stderr);
    }

    #[test]
    fn snapshot_mapping_preserves_all_logging_settings() {
        let mut config = Config::default();
        config.logging.level = "aemeath:tui=debug,aemeath:agent:runtime=trace".to_string();
        config.logging.max_bytes = 42;
        config.logging.max_backups = 3;
        config.logging.retention_days = 14;
        config.logging.logs_dir = Some("custom/logs".to_string());
        let settings = logging_settings_from_snapshot(
            &ConfigSnapshot::new(config),
            Path::new("/fallback/logs"),
            LoggingOutputMode::Stderr,
        );

        assert_eq!(settings.logs_dir(), PathBuf::from("custom/logs"));
        assert_eq!(settings.max_bytes(), 42);
        assert_eq!(settings.max_backups(), 3);
        assert_eq!(settings.retention_days(), 14);
        assert_eq!(settings.output_mode(), LoggingOutputMode::Stderr);
    }

    #[test]
    fn snapshot_mapping_uses_default_logs_dir_when_config_is_absent() {
        let settings = logging_settings_from_snapshot(
            &ConfigSnapshot::new(Config::default()),
            Path::new("/fallback/logs"),
            LoggingOutputMode::File,
        );
        assert_eq!(settings.logs_dir(), PathBuf::from("/fallback/logs"));
    }
}
