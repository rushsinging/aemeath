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

/// 测试用 provider：`invoke` 恒返回有效 reflection JSON（空 output），供反思
/// 端到端路径（生产 `execute_reflection`）驱动到 Succeeded 终态。
pub(crate) struct StaticReflectionProvider;

#[async_trait::async_trait]
impl crate::ports::ProviderPort for StaticReflectionProvider {
    // `capabilities()` 已删除（#1880）：binding 持全量 ModelInfo，运行时零查询。

    async fn invoke(
        &self,
        _request: crate::ports::provider_port::InvocationRequestData,
        _cancel: &dyn crate::ports::provider_port::CancellationSignal,
    ) -> Result<
        crate::ports::provider_port::ProviderResponseStream,
        crate::ports::provider_port::ProviderError,
    > {
        Ok(
            crate::application::model::test_support::text_completion_stream(
                r#"{"deviations":[],"suggested_memories":[],"outdated_memories":[]}"#,
                1,
                1,
            ),
        )
    }
}

/// 与 `StaticReflectionProvider` 配套的 binding（身份/能力与 fake invoke 一致）。
pub(crate) fn static_reflection_binding() -> Arc<crate::ports::ProviderBindingData> {
    Arc::new(crate::ports::ProviderBindingData {
        provider: Arc::new(StaticReflectionProvider),
        model: provider::ModelInfo {
            provider: "reflection-test".to_string(),
            model: "reflection-test-model".to_string(),
            supports_tools: false,
            supports_parallel_tool_calls: false,
            supports_streaming: true,
            reasoning: provider::ReasoningCapabilityData::none(),
            context_limit: Some(128_000),
            output_limit: Some(8_192),
        },
        max_tokens: 8_192,
        requested_reasoning: share::reasoning::ReasoningLevel::Off,
    })
}

/// 记录全部 upsert/append 的 history 替身：list 按插入顺序回放 safe summary
/// （测试自行保证写入顺序），供游标读取等消费逻辑断言。
#[derive(Clone, Default)]
pub(crate) struct RecordingHistory {
    records: Arc<std::sync::Mutex<Vec<memory::api::reflection::ReflectionRecord>>>,
}

#[async_trait::async_trait]
impl memory::api::ReflectionHistoryQuery for RecordingHistory {
    async fn list(
        &self,
        limit: usize,
    ) -> Result<Vec<memory::api::reflection::ReflectionSafeSummary>, memory::api::MemoryError> {
        let records = self.records.lock().unwrap();
        Ok(records
            .iter()
            .rev()
            .take(limit)
            .map(|record| record.safe_summary())
            .collect())
    }
}

#[async_trait::async_trait]
impl memory::api::ReflectionHistoryStore for RecordingHistory {
    async fn append(
        &self,
        record: &memory::api::reflection::ReflectionRecord,
    ) -> Result<(), memory::api::MemoryError> {
        self.records.lock().unwrap().push(record.clone());
        Ok(())
    }

    async fn upsert(
        &self,
        record: &memory::api::reflection::ReflectionRecord,
    ) -> Result<(), memory::api::MemoryError> {
        let mut records = self.records.lock().unwrap();
        if let Some(existing) = records.iter_mut().find(|existing| existing.id == record.id) {
            *existing = record.clone();
        } else {
            records.push(record.clone());
        }
        Ok(())
    }
}
