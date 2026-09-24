use sha2::{Digest, Sha256};
use share::config::domain::merge::{
    AgentsConfigPatch, ApiConfigPatch, ConfigPatch, LoggingConfigPatch, ModelConfigPatch,
    ModelsConfigPatch, PermissionConfigPatch, ToolsConfigPatch, UiConfigPatch,
};
use share::config::hooks::{default_timeout_secs, ClaudeSettingsConfig, HookEntry, HooksConfig};
use share::config::PermissionModeConfig;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use storage::{
    AtomicBlobPort, CommitWarning, Durability, Generation, ReadOutcome, SafePathSegment,
    StorageErrorKind, StorageKey, StorageNamespace, WriteOptions,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigAdapterError {
    PermissionDenied,
    Io,
    Parse,
    Invalid,
    /// 模型选择校验失败，携带可读的根因与可用来源（来自 `ModelResolveError`）。
    InvalidModel {
        detail: String,
    },
    CorruptTransaction,
    UnsupportedDurability,
}

impl std::fmt::Display for ConfigAdapterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PermissionDenied => write!(formatter, "配置目录没有写入权限"),
            Self::Io => write!(formatter, "配置文件读写失败"),
            Self::Parse => write!(formatter, "配置文件格式无效"),
            Self::Invalid => write!(formatter, "配置内容无效"),
            Self::InvalidModel { detail } => write!(formatter, "{detail}"),
            Self::CorruptTransaction => write!(formatter, "配置写入事务损坏"),
            Self::UnsupportedDurability => write!(formatter, "不支持的持久化保证"),
        }
    }
}

impl std::error::Error for ConfigAdapterError {}

pub trait EnvSource: Send + Sync {
    fn get(&self, name: &str) -> Option<String>;
}

pub struct ProcessEnv;

impl EnvSource for ProcessEnv {
    fn get(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

pub struct EnvAdapter;

impl EnvAdapter {
    pub fn read(source: &dyn EnvSource) -> ConfigPatch {
        let global_key = source
            .get("AEMEATH_API_KEY")
            .or_else(|| source.get("LLM_API_KEY"));
        let mut api = ApiConfigPatch {
            provider: source.get("AEMEATH_PROVIDER"),
            key: global_key.clone(),
            base_url: source
                .get("AEMEATH_BASE_URL")
                .or_else(|| source.get("LLM_BASE_URL")),
            ..Default::default()
        };
        if api.provider.as_deref().is_some_and(str::is_empty) {
            api.provider = None;
        }

        let model_name = source.get("AEMEATH_MODEL");
        let model = ModelConfigPatch {
            name: model_name.clone(),
            max_tokens: parse_positive(source.get("AEMEATH_MAX_TOKENS")),
            context_size: parse_positive(source.get("AEMEATH_CONTEXT_SIZE")),
            ..Default::default()
        };
        let mut provider_api_keys = HashMap::new();
        for (driver, env_name) in driver_key_envs() {
            if let Some(key) = source.get(env_name) {
                provider_api_keys.insert(driver.to_string(), key);
            }
        }
        let models = (model_name.is_some()
            || !provider_api_keys.is_empty()
            || global_key.is_some())
        .then(|| ModelsConfigPatch {
            default: model_name,
            provider_api_keys: (!provider_api_keys.is_empty()).then_some(provider_api_keys),
            fallback_api_key: global_key.clone(),
            ..Default::default()
        });
        let permissions = source.get("AEMEATH_PERMISSION_MODE").and_then(|value| {
            let mode = match value.to_ascii_lowercase().as_str() {
                "ask" => PermissionModeConfig::Ask,
                "auto_read" | "autoread" => PermissionModeConfig::AutoRead,
                "allow_all" | "allowall" | "auto_all" | "autoall" => PermissionModeConfig::AllowAll,
                _ => return None,
            };
            Some(PermissionConfigPatch {
                mode: Some(mode),
                ..Default::default()
            })
        });
        let tools = parse_positive(source.get("AEMEATH_MAX_TOOL_CONCURRENCY")).map(|value| {
            ToolsConfigPatch {
                max_concurrency: Some(value),
                ..Default::default()
            }
        });
        let agents = parse_positive(source.get("AEMEATH_MAX_AGENT_CONCURRENCY")).map(|value| {
            AgentsConfigPatch {
                max_concurrency: Some(value),
                ..Default::default()
            }
        });
        let verbose = source.get("AEMEATH_VERBOSE").map(|_| true);
        let color = source.get("NO_COLOR").map(|_| false);
        let ui = (verbose.is_some() || color.is_some()).then_some(UiConfigPatch {
            verbose,
            color,
            ..Default::default()
        });
        let logging = source
            .get("AEMEATH_LOG_LEVEL")
            .map(|level| LoggingConfigPatch {
                level: Some(level),
                ..Default::default()
            });
        let storage = source.get("AEMEATH_WORKTREES_DIR").map(|directory| {
            share::config::domain::merge::StorageConfigPatch {
                worktrees_dir: Some(directory.into()),
                ..Default::default()
            }
        });
        ConfigPatch {
            api: (api.provider.is_some() || api.key.is_some() || api.base_url.is_some())
                .then_some(api),
            model: (model.name.is_some()
                || model.max_tokens.is_some()
                || model.context_size.is_some())
            .then_some(model),
            models,
            permissions,
            tools,
            agents,
            ui,
            logging,
            storage,
            ..Default::default()
        }
    }
}

fn parse_positive<T>(value: Option<String>) -> Option<T>
where
    T: std::str::FromStr + PartialOrd + From<u8>,
{
    value
        .and_then(|value| value.parse::<T>().ok())
        .filter(|value| *value > T::from(0))
}

fn driver_key_envs() -> &'static [(&'static str, &'static str)] {
    &[
        ("anthropic", "ANTHROPIC_API_KEY"),
        ("openai", "OPENAI_API_KEY"),
        ("volcengine", "VOLCENGINE_CODING_PLAN_API_KEY"),
        ("minimax", "MINIMAX_API_KEY"),
        ("mimo", "MIMO_API_KEY"),
        ("deepseek", "DEEPSEEK_API_KEY"),
        ("agnes", "AGNES_API_KEY"),
        ("ollama", "OLLAMA_API_KEY"),
    ]
}

#[derive(Debug, Clone, Default)]
pub struct CliConfigInput {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub max_tokens: Option<u32>,
    pub context_size: Option<usize>,
    pub allow_all: bool,
    pub verbose: bool,
    pub no_markdown: bool,
    pub max_tool_concurrency: Option<usize>,
    pub max_agent_concurrency: Option<usize>,
}

pub struct CliArgsAdapter;

impl CliArgsAdapter {
    pub fn read(input: &CliConfigInput) -> ConfigPatch {
        let api = (input.api_key.is_some() || input.base_url.is_some()).then(|| ApiConfigPatch {
            key: input.api_key.clone(),
            base_url: input.base_url.clone(),
            ..Default::default()
        });
        let model =
            (input.model.is_some() || input.max_tokens.is_some() || input.context_size.is_some())
                .then(|| ModelConfigPatch {
                    name: input.model.clone(),
                    max_tokens: input.max_tokens,
                    context_size: input.context_size.filter(|value| *value > 0),
                    ..Default::default()
                });
        let models =
            (input.model.is_some() || input.api_key.is_some()).then(|| ModelsConfigPatch {
                default: input.model.clone(),
                fallback_api_key: input.api_key.clone(),
                ..Default::default()
            });
        let permissions = input.allow_all.then_some(PermissionConfigPatch {
            mode: Some(PermissionModeConfig::AllowAll),
            ..Default::default()
        });
        let tools = input.max_tool_concurrency.map(|value| ToolsConfigPatch {
            max_concurrency: (value > 0).then_some(value),
            ..Default::default()
        });
        let agents = input.max_agent_concurrency.map(|value| AgentsConfigPatch {
            max_concurrency: (value > 0).then_some(value),
            ..Default::default()
        });
        let ui = (input.verbose || input.no_markdown).then_some(UiConfigPatch {
            verbose: input.verbose.then_some(true),
            markdown: input.no_markdown.then_some(false),
            ..Default::default()
        });
        ConfigPatch {
            api,
            model,
            models,
            permissions,
            tools,
            agents,
            ui,
            ..Default::default()
        }
    }
}

pub struct FileAdapter;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceFingerprints {
    pub global: Option<[u8; 32]>,
    pub claude: Option<[u8; 32]>,
    pub project: Option<[u8; 32]>,
}

pub async fn source_fingerprints(
    global_path: &Path,
    claude_path: Option<&Path>,
    project_path: Option<&Path>,
) -> SourceFingerprints {
    SourceFingerprints {
        global: file_fingerprint(global_path).await,
        claude: match claude_path {
            Some(path) => file_fingerprint(path).await,
            None => None,
        },
        project: match project_path {
            Some(path) => file_fingerprint(path).await,
            None => None,
        },
    }
}

pub fn config_fingerprint(config: &share::config::Config) -> Result<[u8; 32], ConfigAdapterError> {
    fn canonicalize(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.into_iter().map(canonicalize).collect())
            }
            serde_json::Value::Object(values) => {
                let values: BTreeMap<_, _> = values
                    .into_iter()
                    .map(|(key, value)| (key, canonicalize(value)))
                    .collect();
                serde_json::to_value(values).expect("BTreeMap serializes")
            }
            other => other,
        }
    }

    let value = serde_json::to_value(config).map_err(|_| ConfigAdapterError::Parse)?;
    let bytes = serde_json::to_vec(&canonicalize(value)).map_err(|_| ConfigAdapterError::Parse)?;
    Ok(Sha256::digest(bytes).into())
}

async fn file_fingerprint(path: &Path) -> Option<[u8; 32]> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Some(Sha256::digest(bytes).into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            log::warn!(target: crate::LOG_TARGET, "无法读取配置源 {}: {}", path.display(), error);
            Some([0; 32])
        }
    }
}

impl FileAdapter {
    pub async fn read(path: &Path) -> Result<Option<ConfigPatch>, ConfigAdapterError> {
        let content = match tokio::fs::read_to_string(path).await {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                return Err(ConfigAdapterError::PermissionDenied)
            }
            Err(_) => return Err(ConfigAdapterError::Io),
        };
        serde_json::from_str(&content)
            .map(Some)
            .map_err(|_| ConfigAdapterError::Parse)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigFormat {
    ClaudeCode,
    Unknown,
}

pub struct ClaudeTranslator;

impl ClaudeTranslator {
    pub fn looks_like(content: &str) -> bool {
        serde_json::from_str::<serde_json::Value>(content)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .is_some_and(|object| {
                object.contains_key("hooks")
                    || object.contains_key("permissions")
                    || object.contains_key("model")
            })
    }

    pub fn translate(content: &str) -> Result<ConfigPatch, ConfigAdapterError> {
        let value: serde_json::Value =
            serde_json::from_str(content).map_err(|_| ConfigAdapterError::Parse)?;
        let settings: ClaudeSettingsConfig =
            serde_json::from_value(value.clone()).map_err(|_| ConfigAdapterError::Parse)?;
        let hooks = translate_hooks(settings);
        let model_name = value
            .get("model")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let permissions = value.get("permissions").map(|permissions| {
            let allow = permissions
                .get("allow")
                .and_then(serde_json::Value::as_array)
                .map(|allow| {
                    allow
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(ToOwned::to_owned)
                        .collect()
                })
                .filter(|allow: &Vec<String>| !allow.is_empty());
            let deny = permissions
                .get("deny")
                .and_then(serde_json::Value::as_array)
                .map(|deny| {
                    deny.iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(ToOwned::to_owned)
                        .collect()
                })
                .filter(|deny: &Vec<String>| !deny.is_empty());
            // #1469：兼容层只映射 allow/deny 列表，不推断 mode——
            // Config committed permission mode 是唯一模式真相（specs/3.11），
            // 避免项目 .claude/settings.json 的 deny 列表静默覆盖全局显式 mode。
            PermissionConfigPatch {
                mode: None,
                auto_approve: allow,
                deny,
            }
        });
        Ok(ConfigPatch {
            models: model_name.map(|default| ModelsConfigPatch {
                default: Some(default),
                ..Default::default()
            }),
            permissions,
            hooks: (!hooks.events.is_empty()).then_some(hooks),
            ..Default::default()
        })
    }
}

fn translate_hooks(settings: ClaudeSettingsConfig) -> HooksConfig {
    let mut events = HashMap::new();
    for (event, groups) in settings.hooks {
        let mut entries = Vec::new();
        for group in groups {
            for hook in group.hooks {
                if !hook.command.trim().is_empty() {
                    entries.push(HookEntry {
                        matcher: group.matcher.clone(),
                        command: hook.command,
                        timeout: hook.timeout.unwrap_or_else(default_timeout_secs),
                    });
                }
            }
        }
        if !entries.is_empty() {
            events.insert(event, entries);
        }
    }
    HooksConfig {
        events,
        ..HooksConfig::default()
    }
}

pub struct CompatibilityAdapter;

impl CompatibilityAdapter {
    pub fn detect_format(path: &Path, content: &str) -> ConfigFormat {
        if path.file_name().and_then(|name| name.to_str()) == Some("settings.json")
            && (path.to_string_lossy().contains(".claude") || ClaudeTranslator::looks_like(content))
        {
            ConfigFormat::ClaudeCode
        } else {
            ConfigFormat::Unknown
        }
    }

    pub async fn read_one(path: &Path) -> Result<Option<ConfigPatch>, ConfigAdapterError> {
        let content = match tokio::fs::read_to_string(path).await {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                return Err(ConfigAdapterError::PermissionDenied)
            }
            Err(_) => return Err(ConfigAdapterError::Io),
        };
        match Self::detect_format(path, &content) {
            ConfigFormat::ClaudeCode => ClaudeTranslator::translate(&content).map(Some),
            ConfigFormat::Unknown => Ok(None),
        }
    }

    pub async fn read_paths(
        mut paths: Vec<PathBuf>,
    ) -> Result<Vec<ConfigPatch>, ConfigAdapterError> {
        paths.sort();
        let mut patches = Vec::new();
        for path in paths {
            if let Some(patch) = Self::read_one(&path).await? {
                patches.push(patch);
            }
        }
        Ok(patches)
    }
}

pub struct ConfigValidator;

impl ConfigValidator {
    pub fn validate(config: &share::config::Config) -> Result<(), ConfigAdapterError> {
        if config.tools.max_concurrency == 0 || config.agents.max_concurrency == 0 {
            return Err(ConfigAdapterError::Invalid);
        }
        if !config.models.default.is_empty() && !config.models.providers.is_empty() {
            if let Err(error) = config
                .models
                .resolve_model_selection(&config.models.default)
            {
                return Err(ConfigAdapterError::InvalidModel {
                    detail: error.to_string(),
                });
            }
        }
        if reqwest::header::HeaderValue::from_str(&config.api.user_agent).is_err() {
            return Err(ConfigAdapterError::Invalid);
        }
        Ok(())
    }
}

pub fn merge_native_patches(
    base: ConfigPatch,
    overlay: ConfigPatch,
) -> Result<ConfigPatch, ConfigAdapterError> {
    fn merge_value(base: &mut serde_json::Value, overlay: serde_json::Value) {
        match (base, overlay) {
            (_, serde_json::Value::Null) => {}
            (serde_json::Value::Object(base), serde_json::Value::Object(overlay)) => {
                for (key, value) in overlay {
                    merge_value(base.entry(key).or_insert(serde_json::Value::Null), value);
                }
            }
            (base, overlay) => *base = overlay,
        }
    }

    let mut value = serde_json::to_value(base).map_err(|_| ConfigAdapterError::Parse)?;
    let overlay = serde_json::to_value(overlay).map_err(|_| ConfigAdapterError::Parse)?;
    merge_value(&mut value, overlay);
    serde_json::from_value(value).map_err(|_| ConfigAdapterError::Parse)
}

pub fn encode_native_patch(patch: &ConfigPatch) -> Result<Vec<u8>, ConfigAdapterError> {
    serde_json::to_vec(patch).map_err(|_| ConfigAdapterError::Parse)
}

#[derive(Clone)]
pub struct NativeConfigStore {
    storage: Arc<dyn AtomicBlobPort>,
}

impl NativeConfigStore {
    /// 构造仅限 config crate 内部与测试；crate 外一律经 crate 根
    /// `native_override_store` 工厂装配，散落构造在编译期不可达。
    pub(crate) fn new(storage: Arc<dyn AtomicBlobPort>) -> Self {
        Self { storage }
    }

    fn key(project_key: &str) -> Result<StorageKey, ConfigAdapterError> {
        let segment =
            SafePathSegment::from_str(project_key).map_err(|_| ConfigAdapterError::Invalid)?;
        StorageKey::new(StorageNamespace::Config, vec![segment])
            .map_err(|_| ConfigAdapterError::Invalid)
    }

    pub async fn read_override(
        &self,
        project_key: &str,
    ) -> Result<Option<ConfigPatch>, ConfigAdapterError> {
        match self
            .storage
            .read(&Self::key(project_key)?, Generation::Primary)
            .await
            .map_err(map_storage_error)?
        {
            ReadOutcome::NotFound => Ok(None),
            ReadOutcome::Found(blob) => serde_json::from_slice(blob.bytes())
                .map(Some)
                .map_err(|_| ConfigAdapterError::Parse),
        }
    }

    pub async fn write_override(
        &self,
        project_key: &str,
        bytes: &[u8],
    ) -> Result<Option<CommitWarning>, ConfigAdapterError> {
        let receipt = self
            .storage
            .write_atomic(
                &Self::key(project_key)?,
                bytes,
                WriteOptions::new(Durability::ProcessCrashSafe),
            )
            .await
            .map_err(map_storage_error)?;
        Ok(receipt.warning())
    }
}

fn map_storage_error(error: storage::StorageError) -> ConfigAdapterError {
    match error.kind() {
        StorageErrorKind::PermissionDenied => ConfigAdapterError::PermissionDenied,
        StorageErrorKind::UnsupportedDurability => ConfigAdapterError::UnsupportedDurability,
        StorageErrorKind::CorruptTransaction(_) => ConfigAdapterError::CorruptTransaction,
        StorageErrorKind::InvalidKey => ConfigAdapterError::Invalid,
        StorageErrorKind::Io | StorageErrorKind::ConcurrentWrite => ConfigAdapterError::Io,
    }
}

#[cfg(test)]
#[path = "adapters_tests.rs"]
mod tests;

mod app_service;
pub use app_service::ConfigAppService;

#[cfg(test)]
#[path = "adapters/app_service_tests.rs"]
mod app_service_tests;
