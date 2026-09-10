//! Compact 调用模型的动态解析（`context.compact_model`）。
//!
//! Compact 的模型选择有两档语义：
//!
//! 1. 配置了 `context.compact_model` → 使用该 selection 解析出的模型；
//! 2. 未配置（缺省或空串）→ 跟随当前会话模型。
//!
//! 每次 compact 都从 **committed 配置快照**重新解析，因此配置更新与 `/model`
//! 切换在**下一次** compact 生效；解析出的 binding 按 selection 缓存，避免重复
//! 构建。
//!
//! 该类型是"本次 compact 使用哪个模型与窗口"的唯一 owner：生成器与预算填充都
//! 必须经它，**NEVER** 在别处重复解析 selection。

use std::sync::{Arc, Mutex};

use share::config::domain::snapshot::ConfigSnapshot;

use crate::application::client::SessionModelState;
use crate::ports::{ProviderBinding, ProviderFactory};

/// 会话当前模型的共享槽。
///
/// Composition 创建空槽并同时交给 Compact 生成器与 Runtime 装配；Runtime 在
/// 会话装配时把 [`SessionModelState`]（会话模型的唯一真相源）绑定进来。
/// 槽本身不保存第二份模型状态，`/model` 切换通过同一 `SessionModelState` 可见。
#[derive(Clone, Default)]
pub struct SessionModelSlot {
    inner: Arc<std::sync::RwLock<Option<SessionModelState>>>,
}

impl SessionModelSlot {
    pub fn new() -> Self {
        Self::default()
    }

    /// 绑定会话模型真相源；Runtime 在会话装配时调用一次。
    pub(crate) fn bind(&self, state: SessionModelState) {
        *self
            .inner
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(state);
    }

    pub(crate) fn current(&self) -> Option<SessionModelState> {
        self.inner
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

/// Compact 模型的来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactModelOrigin {
    /// 未配置 `context.compact_model`，跟随当前会话模型。
    SessionModel,
    /// 配置了 `context.compact_model`。
    Configured,
}

/// Compact 模型解析失败的 typed 原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactModelResolveError {
    /// 未配置 compact 模型且会话模型尚未绑定。
    SessionModelUnavailable,
    /// 已配置的 selection 无法解析（未知 source/model、缺少 API key 等）。
    Selection(String),
}

impl std::fmt::Display for CompactModelResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SessionModelUnavailable => write!(f, "会话模型尚未绑定"),
            Self::Selection(message) => write!(f, "compact 模型 selection 无法解析：{message}"),
        }
    }
}

impl std::error::Error for CompactModelResolveError {}

/// 本次 compact 应使用的模型绑定。
#[derive(Debug, Clone)]
pub struct CompactModelTarget {
    binding: Arc<ProviderBinding>,
    origin: CompactModelOrigin,
}

impl CompactModelTarget {
    fn new(binding: Arc<ProviderBinding>, origin: CompactModelOrigin) -> Self {
        Self { binding, origin }
    }

    pub fn binding(&self) -> &Arc<ProviderBinding> {
        &self.binding
    }

    pub fn origin(&self) -> CompactModelOrigin {
        self.origin
    }

    /// compact 调用模型的输入窗口；`None` 表示未知，调用方 **MUST** fail closed。
    pub fn context_window(&self) -> Option<usize> {
        self.binding.context_window
    }

    /// 日志用模型标识（`provider/model`），不含 prompt 或凭据。
    pub fn model_identity(&self) -> String {
        format!(
            "{}/{}",
            self.binding.model.provider, self.binding.model.model
        )
    }
}

struct CachedCompactBinding {
    selection: String,
    binding: Arc<ProviderBinding>,
}

/// Compact 模型解析器；见模块文档。
pub struct CompactModelResolver {
    config: Arc<dyn config::ConfigReader>,
    factory: Arc<dyn ProviderFactory>,
    session_model: SessionModelSlot,
    cache: Mutex<Option<CachedCompactBinding>>,
}

impl CompactModelResolver {
    pub fn new(
        config: Arc<dyn config::ConfigReader>,
        factory: Arc<dyn ProviderFactory>,
        session_model: SessionModelSlot,
    ) -> Self {
        Self {
            config,
            factory,
            session_model,
            cache: Mutex::new(None),
        }
    }

    /// 解析本次 compact 使用的模型。
    ///
    /// # Errors
    ///
    /// - 已配置 selection 但无法解析时返回 [`CompactModelResolveError::Selection`]，
    ///   **NEVER** 回退到会话模型；
    /// - 未配置且会话模型未绑定时返回
    ///   [`CompactModelResolveError::SessionModelUnavailable`]。
    pub fn resolve(&self) -> Result<CompactModelTarget, CompactModelResolveError> {
        let snapshot = self.config.committed_snapshot();
        let target = match snapshot.context_compact_model() {
            Some(selection) => {
                let selection = selection.to_string();
                let binding = match self.cached_binding(&selection) {
                    Some(binding) => binding,
                    None => {
                        let binding = self.build_configured_binding(&snapshot, &selection)?;
                        self.store_cached_binding(&selection, Arc::clone(&binding));
                        binding
                    }
                };
                CompactModelTarget::new(binding, CompactModelOrigin::Configured)
            }
            None => {
                let state = self
                    .session_model
                    .current()
                    .ok_or(CompactModelResolveError::SessionModelUnavailable)?;
                CompactModelTarget::new(state.binding(), CompactModelOrigin::SessionModel)
            }
        };
        log::debug!(
            target: crate::LOG_TARGET,
            "[compact] model resolved origin={:?} model={} context_window={:?}",
            target.origin(),
            target.model_identity(),
            target.context_window(),
        );
        Ok(target)
    }

    fn cached_binding(&self, selection: &str) -> Option<Arc<ProviderBinding>> {
        self.cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .filter(|cached| cached.selection == selection)
            .map(|cached| Arc::clone(&cached.binding))
    }

    fn store_cached_binding(&self, selection: &str, binding: Arc<ProviderBinding>) {
        *self.cache.lock().unwrap_or_else(|error| error.into_inner()) =
            Some(CachedCompactBinding {
                selection: selection.to_string(),
                binding,
            });
    }

    fn build_configured_binding(
        &self,
        snapshot: &ConfigSnapshot,
        selection: &str,
    ) -> Result<Arc<ProviderBinding>, CompactModelResolveError> {
        let runtime_model = snapshot
            .resolve_runtime_model(Some(selection), None)
            .map_err(|error| CompactModelResolveError::Selection(error.to_string()))?;
        let (binding, _) = crate::application::client::build_provider_binding_from_runtime_model(
            runtime_model,
            snapshot.api_timeout_secs(),
            snapshot.user_agent(),
            self.factory.as_ref(),
        )
        .map_err(CompactModelResolveError::Selection)?;
        Ok(Arc::new(binding))
    }
}

#[cfg(test)]
#[path = "compact_model_tests.rs"]
mod compact_model_tests;
