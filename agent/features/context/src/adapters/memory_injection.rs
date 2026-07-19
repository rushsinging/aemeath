use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use memory::api::{MemoryPort, MemoryQuery, MemoryRetrievalMode, MemorySearchHit};

use crate::domain::{ContextRequest, SystemBlock};
use crate::ports::{ContextMemorySource, MemoryMaterialization};

/// Read-only bridge from the Memory BC retrieval port into Context system blocks.
pub struct MemoryRetrieveAdapter {
    memory: Arc<dyn MemoryPort>,
    now: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl MemoryRetrieveAdapter {
    pub fn new(memory: Arc<dyn MemoryPort>) -> Self {
        Self::with_clock(memory, Arc::new(system_now))
    }

    /// Constructs the adapter with an injectable Unix-seconds clock.
    pub fn with_clock(
        memory: Arc<dyn MemoryPort>,
        now: Arc<dyn Fn() -> u64 + Send + Sync>,
    ) -> Self {
        Self { memory, now }
    }
}

fn system_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[async_trait]
impl ContextMemorySource for MemoryRetrieveAdapter {
    async fn materialize(&self, request: &ContextRequest) -> Result<MemoryMaterialization, String> {
        let config = request.config_snapshot.memory();
        if !config.enabled || config.inject_count == 0 {
            return Ok(empty_materialization());
        }

        let result = self.memory.retrieve_for_inject(&MemoryQuery {
            limit: config.inject_count,
            layer: None,
            category: None,
            now: (self.now)(),
        });

        match result.mode {
            MemoryRetrievalMode::Disabled => return Ok(empty_materialization()),
            MemoryRetrievalMode::InjectionPriority => {}
            mode => {
                return Err(format!(
                    "memory retrieval returned {mode:?}; expected InjectionPriority"
                ));
            }
        }

        let hits: Vec<_> = result.hits.into_iter().take(config.inject_count).collect();
        if hits.is_empty() {
            return Ok(empty_materialization());
        }

        let content = render_memory_context(&hits);
        Ok(MemoryMaterialization {
            revision: stable_revision(&hits),
            blocks: vec![SystemBlock {
                kind: "memory_context".to_string(),
                content,
                cacheable: false,
            }],
        })
    }
}

fn empty_materialization() -> MemoryMaterialization {
    MemoryMaterialization {
        blocks: Vec::new(),
        revision: 0,
    }
}

fn render_memory_context(hits: &[MemorySearchHit]) -> String {
    let lines = hits.iter().map(|hit| {
        let pinned = if hit.entry.pinned { "★ " } else { "" };
        format!("- {pinned}[{:?}] {}", hit.entry.category, hit.entry.content)
    });
    format!(
        "<memory-context>\n{}\n</memory-context>",
        lines.collect::<Vec<_>>().join("\n")
    )
}

fn stable_revision(hits: &[MemorySearchHit]) -> u64 {
    // FNV-1a is deterministic across processes, unlike `DefaultHasher`.
    let mut revision = 0xcbf29ce484222325_u64;
    for hit in hits {
        for byte in format!(
            "{:?}\0{}\0{}\0",
            hit.entry.category, hit.entry.content, hit.entry.pinned
        )
        .bytes()
        {
            revision ^= u64::from(byte);
            revision = revision.wrapping_mul(0x100000001b3);
        }
    }
    revision
}

pub struct CommittedMemoryRetrieveAdapter {
    memory: Arc<std::sync::RwLock<Arc<dyn MemoryPort>>>,
}

impl CommittedMemoryRetrieveAdapter {
    pub fn new(memory: Arc<std::sync::RwLock<Arc<dyn MemoryPort>>>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl ContextMemorySource for CommittedMemoryRetrieveAdapter {
    async fn materialize(&self, request: &ContextRequest) -> Result<MemoryMaterialization, String> {
        let memory = self
            .memory
            .read()
            .map_err(|error| error.to_string())?
            .clone();
        MemoryRetrieveAdapter::new(memory)
            .materialize(request)
            .await
    }
}

/// Sub Run 或禁用 Memory 时使用的空注入 adapter。
pub struct NoOpContextMemorySource;

#[async_trait]
impl ContextMemorySource for NoOpContextMemorySource {
    async fn materialize(
        &self,
        _request: &ContextRequest,
    ) -> Result<MemoryMaterialization, String> {
        Ok(MemoryMaterialization {
            blocks: Vec::<SystemBlock>::new(),
            revision: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use memory::api::{
        CompactResult, MemoryCategory, MemoryEntry, MemoryError, MemoryId, MemoryLayer,
        MemoryLocation, MemoryPort, MemoryQuery, MemoryRetrievalMode, MemorySearchHit,
        MemorySearchQuery, MemorySearchResult, MemorySource, MemoryStats, ReflectionApplyResult,
        ReflectionOutput, WriteResult,
    };
    use provider::ReasoningLevel;
    use sdk::RunId;
    use share::config::domain::snapshot::ConfigSnapshot;
    use share::config::Config;
    use share::message::Message;

    use super::*;
    use crate::domain::{
        CalendarDate, ContextRequestId, Language, SystemPromptSpec, TaskReminderSnapshot,
    };

    struct FakeMemory {
        result: MemorySearchResult,
        queries: Mutex<Vec<MemoryQuery>>,
    }

    impl FakeMemory {
        fn new(mode: MemoryRetrievalMode, hits: Vec<MemorySearchHit>) -> Self {
            Self {
                result: MemorySearchResult { mode, hits },
                queries: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl MemoryPort for FakeMemory {
        fn retrieve_for_inject(&self, query: &MemoryQuery) -> MemorySearchResult {
            self.queries.lock().unwrap().push(query.clone());
            self.result.clone()
        }

        fn search(&self, _query: &MemorySearchQuery) -> MemorySearchResult {
            panic!("search must not be used for context injection")
        }

        async fn write(&self, _entry: MemoryEntry) -> Result<WriteResult, MemoryError> {
            panic!("context injection must not mutate memory")
        }
        async fn update(&self, _id: &MemoryId, _content: &str) -> Result<bool, MemoryError> {
            panic!("context injection must not mutate memory")
        }
        async fn delete(&self, _id: &MemoryId) -> Result<bool, MemoryError> {
            panic!("context injection must not mutate memory")
        }
        async fn pin(&self, _id: &MemoryId, _pinned: bool) -> Result<bool, MemoryError> {
            panic!("context injection must not mutate memory")
        }
        async fn mark_outdated(&self, _id: &MemoryId) -> Result<bool, MemoryError> {
            panic!("context injection must not mutate memory")
        }
        async fn apply_reflection(
            &self,
            _output: &ReflectionOutput,
        ) -> Result<ReflectionApplyResult, MemoryError> {
            panic!("context injection must not mutate memory")
        }
        async fn archive(&self, _ids: &[MemoryId]) -> Result<(), MemoryError> {
            panic!("context injection must not mutate memory")
        }
        async fn compact(&self) -> Result<CompactResult, MemoryError> {
            panic!("context injection must not mutate memory")
        }
        fn list(&self, _layer: Option<MemoryLayer>) -> Vec<MemoryEntry> {
            panic!("context injection must use retrieve_for_inject")
        }
        fn stats(&self) -> MemoryStats {
            panic!("context injection must use retrieve_for_inject")
        }
    }

    fn hit(id: &str, category: MemoryCategory, content: &str, pinned: bool) -> MemorySearchHit {
        let mut entry = MemoryEntry::new(
            MemoryId::new(id).unwrap(),
            11,
            MemoryLayer::Project,
            category,
            content,
            MemorySource::User,
        )
        .unwrap();
        entry.pinned = pinned;
        entry.ttl = Some(std::time::Duration::from_secs(999));
        MemorySearchHit {
            entry,
            location: MemoryLocation::Archive,
            outdated: true,
            ttl_expired: true,
            relevance: Some(0.987),
        }
    }

    fn request(enabled: bool, inject_count: usize) -> ContextRequest {
        let mut config = Config::default();
        config.memory.enabled = enabled;
        config.memory.inject_count = inject_count;
        ContextRequest {
            session_id: sdk::SessionId::new("session"),
            request_id: ContextRequestId::new("request"),
            run_id: RunId::new("run"),
            step_id: sdk::RunStepId::new("step"),
            pending_messages: vec![Message::user("pending")],
            system_prompt: SystemPromptSpec::new("system"),
            model_id: "fake/model".into(),
            effective_reasoning: ReasoningLevel::Off,
            current_date: CalendarDate::new("2026-07-15"),
            task_reminder: TaskReminderSnapshot::default(),
            language: Language::new("en"),
            agent_roles: HashMap::new(),
            config_snapshot: ConfigSnapshot::new(config),
            context_size: 128_000,
            max_output_tokens: 8_192,
            last_api_input_tokens: None,
            tool_schemas: vec![],
            tool_schema_tokens: 0,
            prev_system_tokens: None,
            prev_tool_schema_tokens: None,
        }
    }

    #[tokio::test]
    async fn preserves_hit_order_and_only_truncates_the_tail() {
        let memory = Arc::new(FakeMemory::new(
            MemoryRetrievalMode::InjectionPriority,
            vec![
                hit(
                    "01890f3c-7c00-7000-8000-000000000001",
                    MemoryCategory::Fact,
                    "first",
                    false,
                ),
                hit(
                    "01890f3c-7c00-7000-8000-000000000002",
                    MemoryCategory::Decision,
                    "second",
                    true,
                ),
                hit(
                    "01890f3c-7c00-7000-8000-000000000003",
                    MemoryCategory::Pattern,
                    "third",
                    false,
                ),
            ],
        ));
        let adapter = MemoryRetrieveAdapter::with_clock(memory.clone(), Arc::new(|| 4242));

        let result = adapter.materialize(&request(true, 2)).await.unwrap();

        assert_eq!(result.blocks.len(), 1);
        assert_eq!(
            result.blocks[0].content,
            "<memory-context>\n- [Fact] first\n- ★ [Decision] second\n</memory-context>"
        );
        assert!(!result.blocks[0].content.contains("third"));
        assert_ne!(result.revision, 0);
        assert_eq!(
            memory.queries.lock().unwrap().as_slice(),
            &[MemoryQuery {
                limit: 2,
                layer: None,
                category: None,
                now: 4242,
            }]
        );
    }

    #[tokio::test]
    async fn excludes_all_hit_and_entry_metadata() {
        let id = "01890f3c-7c00-7000-8000-000000000004";
        let memory = Arc::new(FakeMemory::new(
            MemoryRetrievalMode::InjectionPriority,
            vec![hit(id, MemoryCategory::Preference, "visible only", true)],
        ));
        let adapter = MemoryRetrieveAdapter::with_clock(memory, Arc::new(|| 99));

        let block = &adapter.materialize(&request(true, 5)).await.unwrap().blocks[0];
        assert_eq!(
            block.content,
            "<memory-context>\n- ★ [Preference] visible only\n</memory-context>"
        );
        for forbidden in [id, "0.987", "Archive", "outdated", "ttl", "Project", "User"] {
            assert!(
                !block.content.contains(forbidden),
                "leaked metadata: {forbidden}"
            );
        }
    }

    #[tokio::test]
    async fn disabled_config_returns_empty_without_retrieving() {
        let memory = Arc::new(FakeMemory::new(
            MemoryRetrievalMode::InjectionPriority,
            vec![],
        ));
        let adapter = MemoryRetrieveAdapter::with_clock(memory.clone(), Arc::new(|| 1));

        let result = adapter.materialize(&request(false, 5)).await.unwrap();

        assert!(result.blocks.is_empty());
        assert_eq!(result.revision, 0);
        assert!(memory.queries.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn disabled_result_is_empty_but_other_modes_are_errors() {
        let disabled = MemoryRetrieveAdapter::with_clock(
            Arc::new(FakeMemory::new(MemoryRetrievalMode::Disabled, vec![])),
            Arc::new(|| 1),
        );
        assert!(disabled
            .materialize(&request(true, 5))
            .await
            .unwrap()
            .blocks
            .is_empty());

        let explicit = MemoryRetrieveAdapter::with_clock(
            Arc::new(FakeMemory::new(MemoryRetrievalMode::ExplicitSearch, vec![])),
            Arc::new(|| 1),
        );
        let error = explicit.materialize(&request(true, 5)).await.unwrap_err();
        assert!(error.contains("InjectionPriority"));
        assert!(error.contains("ExplicitSearch"));
    }
}
