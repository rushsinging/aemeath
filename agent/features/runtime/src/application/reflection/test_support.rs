//! Shared test doubles for the reflection stage.

use std::sync::Arc;

/// A `ReflectionHistoryStore` that accepts every write, for tests that care
/// about the run's outcome rather than its durable trace.
pub(crate) fn noop_reflection_history() -> Arc<dyn memory::api::ReflectionHistoryStore> {
    struct NoopHistory;
    #[async_trait::async_trait]
    impl memory::api::ReflectionHistoryQuery for NoopHistory {
        async fn list(
            &self,
            _limit: usize,
        ) -> Result<Vec<memory::api::reflection::ReflectionSafeSummary>, memory::api::MemoryError>
        {
            Ok(Vec::new())
        }
    }
    #[async_trait::async_trait]
    impl memory::api::ReflectionHistoryStore for NoopHistory {
        async fn append(
            &self,
            _record: &memory::api::reflection::ReflectionRecord,
        ) -> Result<(), memory::api::MemoryError> {
            Ok(())
        }
        async fn upsert(
            &self,
            _record: &memory::api::reflection::ReflectionRecord,
        ) -> Result<(), memory::api::MemoryError> {
            Ok(())
        }
    }
    Arc::new(NoopHistory)
}
