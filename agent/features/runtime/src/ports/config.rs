//! Config port — ConfigReader trait for consumers.
//!
//! Defined here (runtime feature) rather than share kernel because
//! async_trait is behavior, and share is pure data + pure functions.

use async_trait::async_trait;
use share::config::domain::snapshot::ConfigSnapshot;
use tokio::sync::watch;

/// Read-only configuration access port.
///
/// Consumers call `snapshot()` for a point-in-time read, or `watch()`
/// to receive push notifications when configuration changes.
#[async_trait]
pub trait ConfigReader: Send + Sync {
    /// Get the current effective configuration snapshot.
    async fn snapshot(&self) -> ConfigSnapshot;

    /// Subscribe to configuration change notifications.
    async fn watch(&self) -> watch::Receiver<ConfigSnapshot>;
}
