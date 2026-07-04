//! ConfigSnapshot — immutable read-only view of merged configuration.
//!
//! Consumers obtain this via the `ConfigReader` port. They NEVER get
//! a mutable reference to `Config`. Field-level accessors expose only
//! what consumers need.

use std::sync::Arc;

use crate::config::permissions::PermissionModeConfig;
use crate::config::Config;

/// Immutable snapshot of effective configuration.
///
/// Wraps `Config` in `Arc` for cheap cloning via `watch::Receiver`.
/// All fields on the inner `Config` are accessed only through accessor
/// methods — consumers cannot mutate or reach the raw `Config`.
#[derive(Debug, Clone)]
pub struct ConfigSnapshot(Arc<Config>);

impl ConfigSnapshot {
    /// Create a new snapshot from a merged `Config`.
    pub fn new(config: Config) -> Self {
        Self(Arc::new(config))
    }

    /// Create a snapshot from an `Arc<Config>` (e.g. from `watch`).
    pub fn from_arc(config: Arc<Config>) -> Self {
        Self(config)
    }

    // ── API ──────────────────────────────────────────────────

    pub fn api_key(&self) -> Option<&str> {
        self.0.api.key.as_deref()
    }

    pub fn base_url(&self) -> Option<&str> {
        self.0.api.base_url.as_deref()
    }

    pub fn provider(&self) -> Option<&str> {
        self.0.api.provider.as_deref()
    }

    // ── Model ────────────────────────────────────────────────

    pub fn model_name(&self) -> &str {
        &self.0.model.name
    }

    pub fn max_tokens(&self) -> u32 {
        self.0.model.max_tokens
    }

    pub fn context_size(&self) -> usize {
        self.0.model.context_size
    }

    // ── Permissions ──────────────────────────────────────────

    pub fn permission_mode(&self) -> PermissionModeConfig {
        self.0.permissions.mode
    }

    pub fn allow_all(&self) -> bool {
        self.0.permissions.mode == PermissionModeConfig::AllowAll
    }

    // ── Tools / Agents ───────────────────────────────────────

    pub fn max_tool_concurrency(&self) -> usize {
        self.0.tools.max_concurrency
    }

    pub fn max_agent_concurrency(&self) -> usize {
        self.0.agents.max_concurrency
    }

    // ── Logging ──────────────────────────────────────────────

    pub fn logging_level(&self) -> &str {
        &self.0.logging.level
    }

    pub fn logs_dir(&self) -> Option<&str> {
        self.0.logging.logs_dir.as_deref()
    }

    // ── UI ───────────────────────────────────────────────────

    pub fn verbose(&self) -> bool {
        self.0.ui.verbose
    }

    pub fn color(&self) -> bool {
        self.0.ui.color
    }

    pub fn markdown(&self) -> bool {
        self.0.ui.markdown
    }

    // ── Memory ───────────────────────────────────────────────

    pub fn memory_enabled(&self) -> bool {
        self.0.memory.enabled
    }

    // ── Storage ──────────────────────────────────────────────

    pub fn persist_sessions(&self) -> bool {
        self.0.storage.persist_sessions
    }

    // ── Guidance ─────────────────────────────────────────────

    pub fn language(&self) -> &str {
        &self.0.language
    }

    // ── Reasoning ────────────────────────────────────────────

    /// Resolve context size with CLI override.
    ///
    /// Priority: CLI explicit (non-zero) > snapshot (env > file already merged) >
    /// provider model context_window > default 128000.
    pub fn resolve_context_size(
        &self,
        cli_override: Option<usize>,
        model_context_window: usize,
    ) -> usize {
        // CLI explicit (non-zero) wins
        if let Some(cli) = cli_override {
            if cli > 0 {
                return cli;
            }
        }
        // snapshot value (already env > file merged)
        if self.0.model.context_size > 0 {
            return self.0.model.context_size;
        }
        // provider model contextWindow
        if model_context_window > 0 {
            return model_context_window;
        }
        // fallback default
        128_000
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_resolve_context_size_cli_wins() {
        let mut config = Config::default();
        config.model.context_size = 32000;
        let snap = ConfigSnapshot::new(config);
        assert_eq!(snap.resolve_context_size(Some(64000), 0), 64000);
    }

    #[test]
    fn test_resolve_context_size_snapshot_wins() {
        let mut config = Config::default();
        config.model.context_size = 32000;
        let snap = ConfigSnapshot::new(config);
        assert_eq!(snap.resolve_context_size(None, 0), 32000);
    }

    #[test]
    fn test_resolve_context_size_model_window_fallback() {
        let config = Config::default();
        let snap = ConfigSnapshot::new(config);
        assert_eq!(snap.resolve_context_size(None, 96000), 96000);
    }

    #[test]
    fn test_resolve_context_size_default() {
        let config = Config::default();
        let snap = ConfigSnapshot::new(config);
        assert_eq!(snap.resolve_context_size(None, 0), 128_000);
    }

    #[test]
    fn test_resolve_context_size_cli_zero_ignored() {
        let mut config = Config::default();
        config.model.context_size = 32000;
        let snap = ConfigSnapshot::new(config);
        assert_eq!(snap.resolve_context_size(Some(0), 0), 32000);
    }
}
