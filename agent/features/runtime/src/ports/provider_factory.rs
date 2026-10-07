//! ProviderFactory — Runtime-owned factory contract for building provider bindings.
//!
//! Composition implements `ProviderFactory` to create `ProviderPort` instances
//! from a `ProviderBuildSpecData` without depending on provider-internal config
//! resolution. The factory owns the provider client construction and capability
//! construction; Runtime only supplies the spec.

use std::sync::Arc;
use std::time::Duration;

use crate::ports::provider_port::{ModelInfo, ProviderError, ProviderPort, ReasoningLevel};

// ─── ProviderBuildSpecData ──────────────────────────────────────

/// Specification sufficient for Composition to construct a provider client
/// via provider config options and wrap it in a `ProviderPort`.
///
/// All fields map directly to provider config options except `context_window`, which
/// feeds the `ModelInfo.context_limit` constructed alongside the client.
#[derive(Debug, Clone)]
pub struct ProviderBuildSpecData {
    /// Driver kind (e.g. `"Anthropic"`, `"OpenAI"`, `"Zhipu"`).
    pub driver: String,
    /// Source key for display / logging.
    pub source_key: String,
    /// API style hint (e.g. `"responses"` for OpenAI Responses API).
    pub api_style: Option<String>,
    /// API key / credential.
    pub api_key: String,
    /// Base URL override.
    pub base_url: Option<String>,
    /// 模型名（模型身份的 provider 侧名——装配时与 source_key 组成
    /// `ModelInfo` 的 provider/model 身份）。
    pub model: String,
    /// Maximum output tokens.
    pub max_tokens: u32,
    /// Requested reasoning level before Provider capability clamp.
    pub requested_reasoning: ReasoningLevel,
    /// Context window size in tokens (`None` = unknown).
    pub context_window: Option<usize>,
    /// Request timeout.
    pub timeout: Duration,
    /// Run-frozen HTTP User-Agent.
    pub user_agent: String,
}

// ─── ProviderBindingData ────────────────────────────────────────

/// An active provider binding: a ready-to-use `ProviderPort` together with the
/// `ModelInfo` and constraints that were used to build it.
#[derive(Clone)]
pub struct ProviderBindingData {
    /// The built provider port.
    pub provider: Arc<dyn ProviderPort>,
    /// 该 binding 绑定模型的完整元数据（身份 + 能力——config/catalog 投影）。
    pub model: ModelInfo,
    /// Maximum output tokens for invocations through this binding.
    pub max_tokens: u32,
    /// Requested reasoning level (before clamping).
    pub requested_reasoning: ReasoningLevel,
}

impl std::fmt::Debug for ProviderBindingData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderBindingData")
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .field("requested_reasoning", &self.requested_reasoning)
            .finish_non_exhaustive()
    }
}

// ─── ProviderFactory trait ──────────────────────────────────

/// Factory that builds a [`ProviderBindingData`] from a [`ProviderBuildSpecData`].
///
/// The factory owns the knowledge of how to construct a provider client and
/// how to construct a `ModelInfo`. The caller (Runtime) only provides the
/// spec — the factory **never** queries external config.
pub trait ProviderFactory: Send + Sync {
    /// Build a provider binding from the given spec.
    ///
    /// # Errors
    ///
    /// Returns `ProviderError` if the spec is invalid (unknown driver, invalid
    /// model, etc.).
    fn build(&self, spec: ProviderBuildSpecData) -> Result<ProviderBindingData, ProviderError>;
}
