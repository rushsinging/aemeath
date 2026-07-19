//! ProviderFactory — Runtime-owned factory contract for building provider bindings.
//!
//! Composition implements `ProviderFactory` to create `ProviderPort` instances
//! from a `ProviderBuildSpec` without depending on provider-internal config
//! resolution. The factory owns the provider client construction and capability
//! construction; Runtime only supplies the spec.

use std::sync::Arc;
use std::time::Duration;

use crate::ports::provider_port::{ModelId, ProviderError, ProviderPort, ReasoningLevel};

// ─── ProviderBuildSpec ──────────────────────────────────────

/// Specification sufficient for Composition to construct a provider client
/// via provider config options and wrap it in a `ProviderPort`.
///
/// All fields map directly to provider config options except `context_window`, which
/// feeds the `ModelCapability.context_limit` constructed alongside the client.
#[derive(Debug, Clone)]
pub struct ProviderBuildSpec {
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
    /// Model identifier.
    pub model: ModelId,
    /// Maximum output tokens.
    pub max_tokens: u32,
    /// Requested reasoning level before Provider capability clamp.
    pub requested_reasoning: ReasoningLevel,
    /// Context window size in tokens (`None` = unknown).
    pub context_window: Option<usize>,
    /// Request timeout.
    pub timeout: Duration,
}

// ─── ProviderBinding ────────────────────────────────────────

/// An active provider binding: a ready-to-use `ProviderPort` together with the
/// model and constraints that were used to build it.
#[derive(Clone)]
pub struct ProviderBinding {
    /// The built provider port.
    pub provider: Arc<dyn ProviderPort>,
    /// Model identifier.
    pub model: ModelId,
    /// Maximum output tokens for invocations through this binding.
    pub max_tokens: u32,
    /// Requested reasoning level (before clamping).
    pub requested_reasoning: ReasoningLevel,
    /// Context window size in tokens (`None` = unknown).
    pub context_window: Option<usize>,
}

impl std::fmt::Debug for ProviderBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderBinding")
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .field("requested_reasoning", &self.requested_reasoning)
            .field("context_window", &self.context_window)
            .finish_non_exhaustive()
    }
}

// ─── ProviderFactory trait ──────────────────────────────────

/// Factory that builds a [`ProviderBinding`] from a [`ProviderBuildSpec`].
///
/// The factory owns the knowledge of how to construct a provider client and
/// how to construct a `ModelCapability`. The caller (Runtime) only provides the
/// spec — the factory **never** queries external config.
pub trait ProviderFactory: Send + Sync {
    /// Build a provider binding from the given spec.
    ///
    /// # Errors
    ///
    /// Returns `ProviderError` if the spec is invalid (unknown driver, invalid
    /// model, etc.).
    fn build(&self, spec: ProviderBuildSpec) -> Result<ProviderBinding, ProviderError>;
}
