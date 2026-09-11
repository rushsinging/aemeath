//! Connect 向导的内存 draft。
//!
//! ## 设计约束
//!
//! - draft 仅存在于 `ConnectAppService` 内存中；View 仅暴露 `has_api_key` 等
//!   投影，绝不暴露 API key 明文；
//! - 字段在更新时做**就地归一化**（空白 / 控制字符剔除），避免后续处理
//!   分裂；
//! - 一旦 `ConfirmSave` 提交失败，旧 draft 仍是服务的内部草稿，不发布新
//!   snapshot；
//! - 字段命名与 `share::config::ProviderModelsConfig` 对齐，便于下一个
//!   Task 通过 `ConnectCommitPort` 桥接到 `GlobalConfigConnectStore`。

use crate::catalog::{DriverId, ProviderSource};

/// 自定义模型草稿。
///
/// 仅在 [`crate::connect::ConnectStage::EditCustomModel`] 阶段被显式构造；
/// 推荐模型选择路径不构造本值对象。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDraft {
    pub model_id: String,
    pub context_window: usize,
    pub max_tokens: u32,
}

impl ModelDraft {
    /// 空白填充校验合法化。
    ///
    /// 拒绝：
    /// - `model_id` 全空白；
    /// - `context_window == 0`；
    /// - `max_tokens == 0` 或 `> context_window`。
    pub fn validate(&self) -> Result<(), DraftValidationError> {
        if self.model_id.trim().is_empty() {
            return Err(DraftValidationError::EmptyModelId);
        }
        if self.context_window == 0 {
            return Err(DraftValidationError::ZeroContextWindow);
        }
        if self.max_tokens == 0 {
            return Err(DraftValidationError::ZeroMaxTokens);
        }
        if (self.max_tokens as usize) > self.context_window {
            return Err(DraftValidationError::MaxTokensExceedsContext {
                context_window: self.context_window,
                max_tokens: self.max_tokens,
            });
        }
        Ok(())
    }
}

/// draft 校验错误。Service 在归一化前调用以拒绝非法值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftValidationError {
    EmptyModelId,
    ZeroContextWindow,
    ZeroMaxTokens,
    MaxTokensExceedsContext {
        context_window: usize,
        max_tokens: u32,
    },
    EmptyBaseUrl,
    InvalidBaseUrlScheme,
    InvalidProviderUserAgent,
}

impl DraftValidationError {
    /// 简短、面向 UI 的描述。
    pub fn message(&self) -> &'static str {
        match self {
            DraftValidationError::EmptyModelId => "模型 ID 不能为空",
            DraftValidationError::ZeroContextWindow => "上下文窗口必须大于 0",
            DraftValidationError::ZeroMaxTokens => "最大输出 token 必须大于 0",
            DraftValidationError::MaxTokensExceedsContext { .. } => {
                "max_tokens 不能超过 context_window"
            }
            DraftValidationError::EmptyBaseUrl => "Base URL 不能为空",
            DraftValidationError::InvalidBaseUrlScheme => {
                "Base URL 必须以 http:// 或 https:// 开头"
            }
            DraftValidationError::InvalidProviderUserAgent => "Provider UA 包含非法字符",
        }
    }
}

/// Credential 状态机：保留显式的"用户已确认保留旧 key"信号。
///
/// - `NotSet`：draft 中还没有任何凭证信息；UI 显示 "无 key"，提交时
///   在 candidate 中写入空 `api_key`，由 Runtime bootstrap 通过
///   EnvAdapter 解析有效凭证。
/// - `UserSet`：用户在向导中输入了新凭证；`api_key` 为非空明文，提交时
///   原样写入 candidate。
/// - `PreservedFromExisting`：用户在 ConfirmOverwrite 路径上保留了已有
///   Provider 的凭证。service 不持有明文；`api_key_plaintext()` 返回
///   `None`。提交时由 commit port 在 global document 中复制已有 `apiKey`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialState {
    NotSet,
    UserSet { api_key: String },
    PreservedFromExisting,
}

impl CredentialState {
    pub fn is_present(&self) -> bool {
        !matches!(self, CredentialState::NotSet)
    }
}

/// Connect 向导的内存草稿。
///
/// 整个对象由 `ConnectAppService` 持有；`view()` 投影为无密钥的
/// [`crate::connect::ConnectDraftView`]，`commit_request()` 投影为 commit
/// 端口输入。
#[derive(Debug, Clone)]
pub struct ConnectDraft {
    pub source: Option<ProviderSource>,
    pub driver: Option<DriverId>,
    pub base_url: Option<String>,
    pub(crate) credential: CredentialState,
    /// Provider 专属 UA 覆盖。`None` 或全空白等同未配置。
    pub provider_user_agent: Option<String>,
    pub model: Option<ModelDraft>,
    pub set_global_default: bool,
}

impl ConnectDraft {
    pub fn empty() -> Self {
        Self {
            source: None,
            driver: None,
            base_url: None,
            credential: CredentialState::NotSet,
            provider_user_agent: None,
            model: None,
            set_global_default: false,
        }
    }

    /// 是否持有凭证（用户输入或确认保留）。View 通过该投影显示。
    /// **NEVER** 暴露明文。
    pub fn has_api_key(&self) -> bool {
        self.credential.is_present()
    }

    /// API key 明文（仅供 service 内部使用）。`PreservedFromExisting`
    /// 路径返回 `None`。外部路径必须走
    /// [`ConnectDraft::has_api_key`] 或 [`ConnectDraftView::has_api_key`]。
    pub(crate) fn api_key_plaintext(&self) -> Option<&str> {
        match &self.credential {
            CredentialState::UserSet { api_key } => Some(api_key.as_str()),
            _ => None,
        }
    }

    /// 用于日志的脱敏投影；绝不返回明文。
    pub fn api_key_redacted(&self) -> Option<&'static str> {
        if self.has_api_key() {
            Some("***redacted***")
        } else {
            None
        }
    }

    /// 用户设置新凭证时调用；新值覆盖 `PreservedFromExisting`。
    pub(crate) fn set_user_credential(&mut self, raw: String) {
        self.credential = if raw.is_empty() {
            CredentialState::NotSet
        } else {
            CredentialState::UserSet { api_key: raw }
        };
    }

    /// 在 ConfirmOverwrite 路径上确认保留旧 key。
    pub(crate) fn preserve_existing_credential(&mut self) {
        if matches!(self.credential, CredentialState::NotSet) {
            self.credential = CredentialState::PreservedFromExisting;
        }
        // UserSet 状态保留用户最新输入，不替换。
    }

    /// Validate `base_url` 进入 draft 的归一化函数。空字符串、全空白或非
    /// `http(s)` scheme 一律拒绝。
    pub(crate) fn normalize_base_url(raw: &str) -> Result<String, DraftValidationError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(DraftValidationError::EmptyBaseUrl);
        }
        if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
            return Err(DraftValidationError::InvalidBaseUrlScheme);
        }
        Ok(trimmed.to_string())
    }

    /// Provider UA 归一化：空白 → 清除（合法）；含控制字符 → Validation 错误。
    pub(crate) fn normalize_provider_user_agent(raw: Option<&str>) -> Option<String> {
        let raw = raw?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(trimmed.to_string())
    }

    /// Provider UA 校验：含控制字符时返回 [`DraftValidationError::InvalidProviderUserAgent`]。
    pub(crate) fn validate_provider_user_agent(
        raw: Option<&str>,
    ) -> Result<Option<String>, DraftValidationError> {
        let normalized = Self::normalize_provider_user_agent(raw);
        if let Some(value) = normalized.as_deref() {
            if value.bytes().any(|byte| byte.is_ascii_control()) {
                return Err(DraftValidationError::InvalidProviderUserAgent);
            }
        }
        Ok(normalized)
    }
}

#[cfg(test)]
#[path = "draft_tests.rs"]
mod draft_tests;
