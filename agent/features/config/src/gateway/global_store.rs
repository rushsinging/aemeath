//! Config-owned global document durable store。

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::connect::ConnectDraft;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GlobalConfigRevision(String);

impl GlobalConfigRevision {
    pub fn from_digest(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct GlobalConfigDocument {
    pub revision: GlobalConfigRevision,
    pub value: Value,
}

#[derive(Debug, Clone)]
pub struct BootstrapConfigReceipt {
    pub(crate) path: PathBuf,
    pub(crate) digest: GlobalConfigRevision,
}

#[derive(Debug, Error)]
pub(crate) enum GlobalConfigStoreError {
    #[error("全局配置已存在")]
    AlreadyExists,
    #[error("全局配置已被其他进程修改")]
    Conflict { expected: GlobalConfigRevision },
    #[error("全局配置不是合法 JSON 对象：{0}")]
    InvalidDocument(String),
    #[error("全局配置写入失败：{0}")]
    Io(String),
    #[error("启动配置已被外部修改，拒绝回滚")]
    RollbackRefused,
    #[error("Connect 草稿缺少字段：{0}")]
    InvalidDraft(&'static str),
}

#[async_trait::async_trait]
pub trait GlobalConfigConnectStore: Send + Sync {
    async fn load_global_document(
        &self,
    ) -> Result<Option<GlobalConfigDocument>, share::error::DomainError>;
    async fn create_complete_default(
        &self,
    ) -> Result<BootstrapConfigReceipt, share::error::DomainError>;
    async fn compare_and_swap(
        &self,
        expected: GlobalConfigRevision,
        draft: ConnectDraft,
    ) -> Result<GlobalConfigRevision, share::error::DomainError>;
    async fn rollback_bootstrap(
        &self,
        receipt: BootstrapConfigReceipt,
    ) -> Result<(), share::error::DomainError>;
}

impl From<GlobalConfigStoreError> for share::error::DomainError {
    fn from(error: GlobalConfigStoreError) -> Self {
        let folded = match &error {
            GlobalConfigStoreError::Io(message) => {
                share::error::DomainError::storage("config", message.clone())
            }
            GlobalConfigStoreError::Conflict { .. }
            | GlobalConfigStoreError::AlreadyExists
            | GlobalConfigStoreError::InvalidDocument(_)
            | GlobalConfigStoreError::InvalidDraft(_)
            | GlobalConfigStoreError::RollbackRefused => {
                share::error::DomainError::invalid("config", error.to_string())
            }
        };
        // 内部变体挂入 source 链：跨界只暴露分类 + 文案，crate 内与
        // `is_persist_conflict` 经 `source_downcast_ref` 保留可识别性。
        folded.with_source(std::sync::Arc::new(error))
    }
}

/// CAS 冲突（`Conflict` 变体）的行为判定：错误折叠为 `DomainError` 后，
/// 消费方（composition 的 commit 适配器）无须 match 内部变体即可分流，
/// 冲突语义由 config crate 独占解释。
pub fn is_persist_conflict(error: &share::error::DomainError) -> bool {
    error
        .source_downcast_ref::<GlobalConfigStoreError>()
        .map(|inner| matches!(inner, GlobalConfigStoreError::Conflict { .. }))
        .unwrap_or(false)
}

pub(crate) struct FilesystemGlobalConfigConnectStore {
    agents_dir: PathBuf,
}

impl FilesystemGlobalConfigConnectStore {
    pub(crate) fn new(agents_dir: PathBuf) -> Self {
        Self { agents_dir }
    }

    pub(crate) fn config_path(&self) -> PathBuf {
        self.agents_dir.join("aemeath.json")
    }

    fn lock_path(&self) -> PathBuf {
        self.agents_dir.join("aemeath.json.lock")
    }

    fn with_lock<T>(
        &self,
        operation: impl FnOnce() -> Result<T, GlobalConfigStoreError>,
    ) -> Result<T, GlobalConfigStoreError> {
        fs::create_dir_all(&self.agents_dir).map_err(map_io)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.lock_path())
            .map_err(map_io)?;
        lock.lock_exclusive().map_err(map_io)?;
        let result = operation();
        let unlock_result = fs2::FileExt::unlock(&lock).map_err(map_io);
        match (result, unlock_result) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }

    pub(crate) fn load_global_document(
        &self,
    ) -> Result<Option<GlobalConfigDocument>, GlobalConfigStoreError> {
        self.with_lock(|| self.load_unlocked())
    }

    pub(crate) fn create_complete_default(
        &self,
    ) -> Result<BootstrapConfigReceipt, GlobalConfigStoreError> {
        self.with_lock(|| {
            let path = self.config_path();
            let bytes = serde_json::to_vec_pretty(&share::config::Config::default())
                .map_err(|error| GlobalConfigStoreError::InvalidDocument(error.to_string()))?;
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .map_err(|error| {
                    if error.kind() == io::ErrorKind::AlreadyExists {
                        GlobalConfigStoreError::AlreadyExists
                    } else {
                        map_io(error)
                    }
                })?;
            file.write_all(&bytes).map_err(map_io)?;
            file.sync_all().map_err(map_io)?;
            sync_directory(&self.agents_dir)?;
            Ok(BootstrapConfigReceipt {
                path,
                digest: digest(&bytes),
            })
        })
    }

    pub(crate) fn commit_draft(
        &self,
        expected: GlobalConfigRevision,
        draft: &ConnectDraft,
    ) -> Result<GlobalConfigRevision, GlobalConfigStoreError> {
        self.with_lock(|| {
            let loaded = self.load_unlocked()?.ok_or_else(|| {
                GlobalConfigStoreError::InvalidDocument("全局配置不存在".to_string())
            })?;
            if loaded.revision != expected {
                return Err(GlobalConfigStoreError::Conflict { expected });
            }
            let candidate = merge_draft(loaded.value, draft)?;
            let bytes = serde_json::to_vec_pretty(&candidate)
                .map_err(|error| GlobalConfigStoreError::InvalidDocument(error.to_string()))?;
            atomic_replace(&self.config_path(), &bytes)?;
            Ok(digest(&bytes))
        })
    }

    pub(crate) fn rollback_bootstrap(
        &self,
        receipt: &BootstrapConfigReceipt,
    ) -> Result<(), GlobalConfigStoreError> {
        self.with_lock(|| {
            let path = self.config_path();
            if receipt.path != path {
                return Err(GlobalConfigStoreError::RollbackRefused);
            }
            let bytes = fs::read(&path).map_err(map_io)?;
            if digest(&bytes) != receipt.digest {
                return Err(GlobalConfigStoreError::RollbackRefused);
            }
            fs::remove_file(path).map_err(map_io)?;
            sync_directory(&self.agents_dir)
        })
    }

    fn load_unlocked(&self) -> Result<Option<GlobalConfigDocument>, GlobalConfigStoreError> {
        let path = self.config_path();
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(map_io(error)),
        };
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|error| GlobalConfigStoreError::InvalidDocument(error.to_string()))?;
        if !value.is_object() {
            return Err(GlobalConfigStoreError::InvalidDocument(
                "根节点必须是对象".to_string(),
            ));
        }
        Ok(Some(GlobalConfigDocument {
            revision: digest(&bytes),
            value,
        }))
    }
}

#[async_trait::async_trait]
impl GlobalConfigConnectStore for FilesystemGlobalConfigConnectStore {
    async fn load_global_document(
        &self,
    ) -> Result<Option<GlobalConfigDocument>, share::error::DomainError> {
        FilesystemGlobalConfigConnectStore::load_global_document(self).map_err(Into::into)
    }

    async fn create_complete_default(
        &self,
    ) -> Result<BootstrapConfigReceipt, share::error::DomainError> {
        FilesystemGlobalConfigConnectStore::create_complete_default(self).map_err(Into::into)
    }

    async fn compare_and_swap(
        &self,
        expected: GlobalConfigRevision,
        draft: ConnectDraft,
    ) -> Result<GlobalConfigRevision, share::error::DomainError> {
        self.commit_draft(expected, &draft).map_err(Into::into)
    }

    async fn rollback_bootstrap(
        &self,
        receipt: BootstrapConfigReceipt,
    ) -> Result<(), share::error::DomainError> {
        FilesystemGlobalConfigConnectStore::rollback_bootstrap(self, &receipt).map_err(Into::into)
    }
}

fn merge_draft(mut root: Value, draft: &ConnectDraft) -> Result<Value, GlobalConfigStoreError> {
    let source = draft
        .source
        .clone()
        .ok_or(GlobalConfigStoreError::InvalidDraft("source"))?;
    let driver = draft
        .driver
        .ok_or(GlobalConfigStoreError::InvalidDraft("driver"))?;
    let base_url = draft
        .base_url
        .as_ref()
        .ok_or(GlobalConfigStoreError::InvalidDraft("base_url"))?;
    if draft.models.is_empty() {
        return Err(GlobalConfigStoreError::InvalidDraft("model"));
    }
    let root = root
        .as_object_mut()
        .ok_or_else(|| GlobalConfigStoreError::InvalidDocument("根节点必须是对象".to_string()))?;
    let models = object_field(root, "models")?;
    let providers = object_field(models, "providers")?;
    let existing_key = providers
        .keys()
        .find(|key| key.as_str() == source.as_str())
        .cloned();
    let existing_api_key = existing_key
        .as_ref()
        .and_then(|key| providers.get(key))
        .and_then(|value| value.get("apiKey"))
        .cloned();
    let mut provider = Map::new();
    provider.insert("baseUrl".to_string(), Value::String(base_url.clone()));
    provider.insert(
        "driver".to_string(),
        Value::String(driver.as_str().to_string()),
    );
    provider.insert(
        "apiKey".to_string(),
        draft
            .api_key_plaintext()
            .map(|key| Value::String(key.to_string()))
            .or(existing_api_key)
            .unwrap_or_else(|| Value::String(String::new())),
    );
    let model_values: Vec<Value> = draft
        .models
        .iter()
        .map(|model| {
            let mut entry = serde_json::json!({
                "id": model.model_id,
                "name": model.model_id,
                "input": ["text"],
                "contextWindow": model.context_window,
                "max_tokens": model.max_tokens,
            });
            if let Some(effort) = model
                .reasoning_effort
                .as_ref()
                .filter(|value| !value.trim().is_empty())
            {
                entry["reasoningEffort"] = Value::String(effort.trim().to_string());
            }
            entry
        })
        .collect();
    provider.insert("models".to_string(), Value::Array(model_values));
    if let Some(user_agent) = draft
        .provider_user_agent
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        provider.insert(
            "userAgent".to_string(),
            Value::String(user_agent.trim().to_string()),
        );
    }
    if let Some(api_style) = draft
        .api_style
        .as_ref()
        .filter(|style| !style.trim().is_empty())
    {
        provider.insert(
            "apiStyle".to_string(),
            Value::String(api_style.trim().to_string()),
        );
    }
    if let Some(key) = existing_key {
        providers.remove(&key);
    }
    providers.insert(source.as_str().to_string(), Value::Object(provider));
    // 全局默认模型：编辑页"设为全局默认"勾选产生（全局唯一）；
    // 未显式指定时回落首个所选模型。
    let default_model = draft
        .default_model_id
        .as_deref()
        .and_then(|target| {
            draft
                .models
                .iter()
                .find(|model| model.model_id == target)
                .map(|model| model.model_id.as_str())
        })
        .or_else(|| draft.models.first().map(|model| model.model_id.as_str()));
    if let Some(default_model) = default_model {
        models.insert(
            "default".to_string(),
            Value::String(format!("{}/{}", source.as_str(), default_model)),
        );
    }
    Ok(root.clone().into())
}

fn object_field<'a>(
    parent: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>, GlobalConfigStoreError> {
    let value = parent
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    value
        .as_object_mut()
        .ok_or_else(|| GlobalConfigStoreError::InvalidDocument(format!("{key} 必须是对象")))
}

fn digest(bytes: &[u8]) -> GlobalConfigRevision {
    GlobalConfigRevision(format!("{:x}", Sha256::digest(bytes)))
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), GlobalConfigStoreError> {
    let parent = path
        .parent()
        .ok_or_else(|| GlobalConfigStoreError::Io("配置路径缺少父目录".to_string()))?;
    let tmp = parent.join(format!(".aemeath.json.{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp)
            .map_err(map_io)?;
        file.write_all(bytes).map_err(map_io)?;
        file.sync_all().map_err(map_io)?;
        fs::rename(&tmp, path).map_err(map_io)?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}

fn sync_directory(path: &Path) -> Result<(), GlobalConfigStoreError> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(map_io)
}

fn map_io(error: io::Error) -> GlobalConfigStoreError {
    GlobalConfigStoreError::Io(error.to_string())
}

#[cfg(test)]
#[path = "global_store_tests.rs"]
mod tests;
