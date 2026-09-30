use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use memory::api::search::{MemoryRetrievalMode, MemorySearchHit};
use memory::api::{MemoryPort, MemoryQuery};

use crate::domain::{ContextRequestData, SystemBlock};
use crate::ports::{ContextMemorySource, MemoryMaterialization};

/// 注入候选的检索窗口上限。token 预算才是真正的约束（#1777 移除了条数
/// 上限），这里只保证排序与让位顺序有足够素材。
const INJECTION_CANDIDATE_LIMIT: usize = 200;

/// Read-only bridge from the Memory BC retrieval port into Context system blocks.
pub(crate) struct MemoryRetrieveAdapter {
    memory: Arc<dyn MemoryPort>,
    now: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl MemoryRetrieveAdapter {
    pub fn new(memory: Arc<dyn MemoryPort>) -> Self {
        Self::with_clock(memory, Arc::new(system_now))
    }

    /// 按窗口比例物化注入内容。`context_size` 来自当轮 request——预算随窗口
    /// 缩放，大窗口不再浪费空间、小窗口也不会超出比例（#1777）。
    pub async fn materialize_config(
        &self,
        config: &share::config::MemoryConfig,
        context_size: usize,
    ) -> Result<MemoryMaterialization, String> {
        let token_budget = match config.inject_token_budget {
            Some(0) => return Ok(empty_materialization()),
            Some(fixed) => fixed,
            None => crate::domain::token_budget::injection_token_budget(context_size),
        };
        if !config.enabled || token_budget == 0 {
            return Ok(empty_materialization());
        }

        let result = self.memory.retrieve_for_inject(&MemoryQuery {
            limit: INJECTION_CANDIDATE_LIMIT,
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

        // 条数上限已移除（#1777）：token 预算本身就是上限。候选仍取一个
        // 足够大的窗口，保证排序与让位顺序完整，截断交给预算。
        let candidate_count = result.hits.len();
        let hits = take_with_overlay_within_budget(result.hits, token_budget);
        let estimated_tokens = hits
            .iter()
            .map(|hit| crate::domain::token_budget::estimate_tokens(&render_memory_line(hit)))
            .sum::<usize>();
        let global_hits = hits
            .iter()
            .filter(|hit| hit.entry.layer == memory::api::MemoryLayer::Global)
            .count();
        let project_hits = hits.len().saturating_sub(global_hits);
        log::debug!(
            target: crate::LOG_TARGET,
            "memory_injection_materialized candidates={} injected={} estimated_tokens={} dropped={} token_budget={} context_size={} global_hits={} project_hits={}",
            candidate_count,
            hits.len(),
            estimated_tokens,
            candidate_count.saturating_sub(hits.len()),
            token_budget,
            context_size,
            global_hits,
            project_hits
        );
        if hits.is_empty() {
            return Ok(empty_materialization());
        }

        let content = render_memory_context(&hits);
        Ok(MemoryMaterialization {
            revision: stable_revision(&hits),
            blocks: vec![SystemBlock {
                kind: "memory_context".to_string(),
                content,
                cacheable: true,
                cache_break: false,
            }],
        })
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
    async fn materialize(
        &self,
        request: &ContextRequestData,
    ) -> Result<MemoryMaterialization, String> {
        self.materialize_config(request.config_snapshot.memory(), request.context_size)
            .await
    }
}

fn empty_materialization() -> MemoryMaterialization {
    MemoryMaterialization {
        blocks: Vec::new(),
        revision: 0,
    }
}

/// 覆盖式让位的注入填充（#1777）。
///
/// Memory 侧已把 `kind = Synthesized` 的结论排到全部普通条目之前，组内保持
/// `injection_score` 顺序（不改分数，只改顺序）。这里做两段预算填充：
///
/// 1. 先填结论段；
/// 2. 再用剩余预算填普通条目，但**跳过被已选结论覆盖的来源**。
///
/// 「已覆盖」在填充时计算（扫描已选结论的 `evidence`），不依赖任何写时标记。
/// 关键性质：结论若在第一段就没挤进预算，它的来源在第二段照常参与——固定
/// 降权系数会在这个场景误伤来源，让位因此必须是条件性的。
fn take_with_overlay_within_budget(
    hits: Vec<MemorySearchHit>,
    token_budget: usize,
) -> Vec<MemorySearchHit> {
    let mut used_tokens = 0usize;
    let mut selected = Vec::new();
    let mut covered = std::collections::HashSet::new();

    // 段一：结论（Memory 侧已排在前），超预算即停——被挤掉的结论不产生覆盖。
    for hit in hits
        .iter()
        .filter(|hit| hit.entry.kind == memory::api::MemoryKind::Synthesized)
        .cloned()
    {
        if !fill_within_budget(&hit, token_budget, &mut used_tokens, &mut covered) {
            break;
        }
        selected.push(hit);
    }
    // 段二：其余条目，被已选结论覆盖的来源让位。
    for hit in hits
        .into_iter()
        .filter(|hit| hit.entry.kind != memory::api::MemoryKind::Synthesized)
    {
        if covered.contains(&hit.entry.id) {
            continue;
        }
        if !fill_within_budget(&hit, token_budget, &mut used_tokens, &mut covered) {
            break;
        }
        selected.push(hit);
    }
    selected
}

/// 单条填充：预算够则计入并（若是结论）登记其覆盖的来源，返回是否入账。
fn fill_within_budget(
    hit: &MemorySearchHit,
    token_budget: usize,
    used_tokens: &mut usize,
    covered: &mut std::collections::HashSet<memory::api::MemoryId>,
) -> bool {
    let tokens = crate::domain::token_budget::estimate_tokens(&render_memory_line(hit));
    if used_tokens.saturating_add(tokens) > token_budget {
        return false;
    }
    *used_tokens = used_tokens.saturating_add(tokens);
    if hit.entry.kind == memory::api::MemoryKind::Synthesized {
        covered.extend(hit.entry.evidence.iter().copied());
    }
    true
}

fn render_memory_line(hit: &MemorySearchHit) -> String {
    let pinned = if hit.entry.pinned { "★ " } else { "" };
    format!("- {pinned}[{:?}] {}", hit.entry.category, hit.entry.content)
}

fn render_memory_context(hits: &[MemorySearchHit]) -> String {
    let lines = hits.iter().map(render_memory_line);
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

pub(crate) struct CommittedMemoryRetrieveAdapter {
    memory: Arc<std::sync::RwLock<Arc<dyn MemoryPort>>>,
}

impl CommittedMemoryRetrieveAdapter {
    pub fn new(memory: Arc<std::sync::RwLock<Arc<dyn MemoryPort>>>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl ContextMemorySource for CommittedMemoryRetrieveAdapter {
    async fn materialize(
        &self,
        request: &ContextRequestData,
    ) -> Result<MemoryMaterialization, String> {
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
pub(crate) struct NoOpContextMemorySource;

#[async_trait]
impl ContextMemorySource for NoOpContextMemorySource {
    async fn materialize(
        &self,
        _request: &ContextRequestData,
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
    use memory::api::reflection::{ReflectionApplyResult, ReflectionOutput};
    use memory::api::search::{
        MemoryRetrievalMode, MemorySearchHit, MemorySearchQuery, MemorySearchResult,
    };
    use memory::api::{
        CompactResult, MemoryCategory, MemoryEntry, MemoryError, MemoryId, MemoryLayer,
        MemoryLocation, MemoryPort, MemoryQuery, MemorySource, MemoryStats, WriteResult,
    };
    use sdk::RunId;
    use share::config::domain::snapshot::ConfigSnapshot;
    use share::config::Config;
    use share::message::Message;
    use share::reasoning::ReasoningLevel;

    use super::*;
    use crate::domain::{ContextRequestId, Language, SystemPromptSpecData};

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
        async fn archive(&self, _ids: &[MemoryId]) -> Result<bool, MemoryError> {
            panic!("context injection must not mutate memory")
        }
        async fn restore(&self, _id: &MemoryId) -> Result<memory::api::RestoreResult, MemoryError> {
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
            superseded_by: None,
            relevance: Some(0.987),
        }
    }

    /// 归纳结论命中：携带 `evidence` 来源。
    fn synthesized_hit(id: &str, content: &str, sources: Vec<MemoryId>) -> MemorySearchHit {
        let mut hit = hit(id, MemoryCategory::Decision, content, false);
        hit.entry.kind = memory::api::MemoryKind::Synthesized;
        hit.entry.evidence = sources;
        hit
    }

    #[test]
    fn a_selected_conclusion_pushes_its_sources_out_of_the_injection() {
        let source_a = MemoryId::new("01890f3c-7c00-7000-8000-0000000000a1").unwrap();
        let source_b = MemoryId::new("01890f3c-7c00-7000-8000-0000000000a2").unwrap();
        let conclusion_id = MemoryId::new("01890f3c-7c00-7000-8000-0000000000b1").unwrap();
        let hits = vec![
            synthesized_hit(
                "01890f3c-7c00-7000-8000-0000000000b1",
                "release ownership is centralised",
                vec![source_a, source_b],
            ),
            hit(
                "01890f3c-7c00-7000-8000-0000000000a1",
                MemoryCategory::Fact,
                "a source fact",
                false,
            ),
            hit(
                "01890f3c-7c00-7000-8000-0000000000a2",
                MemoryCategory::Fact,
                "another source fact",
                false,
            ),
        ];

        let selected = take_with_overlay_within_budget(hits, 4_000);

        assert_eq!(
            selected.len(),
            1,
            "the conclusion takes the whole budget slot"
        );
        assert_eq!(selected[0].entry.id, conclusion_id);
    }

    #[test]
    fn a_conclusion_that_loses_the_budget_leaves_its_sources_alone() {
        let source = MemoryId::new("01890f3c-7c00-7000-8000-0000000000a1").unwrap();
        let hits = vec![
            synthesized_hit(
                "01890f3c-7c00-7000-8000-0000000000b1",
                "a conclusion long enough to blow a tiny budget on its own",
                vec![source],
            ),
            hit(
                "01890f3c-7c00-7000-8000-0000000000a1",
                MemoryCategory::Fact,
                "short source",
                false,
            ),
        ];

        let selected = take_with_overlay_within_budget(hits, 8);

        assert!(
            !selected
                .iter()
                .any(|hit| hit.entry.kind == memory::api::MemoryKind::Synthesized),
            "the oversized conclusion does not fit"
        );
        assert_eq!(
            selected.len(),
            1,
            "its source must still be injected — downweighting would punish it for nothing"
        );
        assert_eq!(selected[0].entry.id, source);
    }

    #[test]
    fn an_uncovered_source_fills_the_budget_left_by_the_conclusion() {
        let covered = MemoryId::new("01890f3c-7c00-7000-8000-0000000000a1").unwrap();
        let other = MemoryId::new("01890f3c-7c00-7000-8000-0000000000a2").unwrap();
        let hits = vec![
            synthesized_hit(
                "01890f3c-7c00-7000-8000-0000000000b1",
                "release ownership is centralised",
                vec![covered],
            ),
            hit(
                "01890f3c-7c00-7000-8000-0000000000a1",
                MemoryCategory::Fact,
                "a covered source",
                false,
            ),
            hit(
                "01890f3c-7c00-7000-8000-0000000000a2",
                MemoryCategory::Fact,
                "an unrelated fact",
                false,
            ),
        ];

        let selected = take_with_overlay_within_budget(hits, 4_000);
        let ids = selected.iter().map(|hit| hit.entry.id).collect::<Vec<_>>();

        assert!(
            ids.contains(&other),
            "uncovered entries still get their share"
        );
        assert!(!ids.contains(&covered), "the covered source steps aside");
    }

    /// `inject_token_budget` 为 `None` 时走窗口比例（#1777）；传 `Some(n)`
    /// 覆盖为固定预算（`Some(0)` 禁用）。
    fn request(enabled: bool, inject_token_budget: Option<usize>) -> ContextRequestData {
        let mut config = Config::default();
        config.memory.enabled = enabled;
        config.memory.inject_token_budget = inject_token_budget;
        ContextRequestData {
            session_id: sdk::SessionId::new("session"),
            request_id: ContextRequestId::new("request"),
            run_id: RunId::new("run"),
            step_id: sdk::RunStepId::new("step"),
            pending_messages: vec![Message::user("pending")],
            invocation_reminders: vec![],
            system_prompt: SystemPromptSpecData::new("system"),
            model_id: "fake/model".into(),
            effective_reasoning: ReasoningLevel::Off,
            language: Language::new("en"),
            agent_roles: HashMap::new(),
            config_snapshot: ConfigSnapshot::new(config),
            context_size: 128_000,
            max_output_tokens: 8_192,
            last_api_total_tokens: None,
            heuristic_calibration: None,
            tool_schemas: vec![],
            tool_schema_tokens: 0,
        }
    }

    /// #1777：条数上限移除后，只要预算允许，全部命中按序进入；截断只由
    /// token 预算决定（见 `token_budget_keeps_only_the_ordered_prefix`）。
    #[tokio::test]
    async fn preserves_hit_order_within_the_budget() {
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

        let result = adapter
            .materialize(&request(true, Some(300)))
            .await
            .unwrap();

        assert_eq!(result.blocks.len(), 1);
        assert_eq!(
            result.blocks[0].content,
            "<memory-context>\n- [Fact] first\n- ★ [Decision] second\n- [Pattern] third\n</memory-context>"
        );
        assert_ne!(result.revision, 0);
        assert_eq!(
            memory.queries.lock().unwrap().as_slice(),
            &[MemoryQuery {
                limit: super::INJECTION_CANDIDATE_LIMIT,
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

        let block = &adapter
            .materialize(&request(true, Some(300)))
            .await
            .unwrap()
            .blocks[0];
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

        let result = adapter
            .materialize(&request(false, Some(300)))
            .await
            .unwrap();

        assert!(result.blocks.is_empty());
        assert_eq!(result.revision, 0);
        assert!(memory.queries.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn token_budget_keeps_only_the_ordered_prefix() {
        let memory = Arc::new(FakeMemory::new(
            MemoryRetrievalMode::InjectionPriority,
            vec![
                hit(
                    "01890f3c-7c00-7000-8000-000000000011",
                    MemoryCategory::Fact,
                    "short first",
                    false,
                ),
                hit(
                    "01890f3c-7c00-7000-8000-000000000012",
                    MemoryCategory::Decision,
                    "second entry is deliberately much longer than the remaining budget",
                    false,
                ),
                hit(
                    "01890f3c-7c00-7000-8000-000000000013",
                    MemoryCategory::Pattern,
                    "tiny third",
                    false,
                ),
            ],
        ));
        let adapter = MemoryRetrieveAdapter::with_clock(memory, Arc::new(|| 1));

        let result = adapter.materialize(&request(true, Some(8))).await.unwrap();
        let content = &result.blocks[0].content;

        assert!(content.contains("short first"));
        assert!(!content.contains("second entry"));
        assert!(!content.contains("tiny third"));
    }

    #[tokio::test]
    async fn zero_token_budget_disables_injection_without_retrieving() {
        let memory = Arc::new(FakeMemory::new(
            MemoryRetrievalMode::InjectionPriority,
            vec![],
        ));
        let adapter = MemoryRetrieveAdapter::with_clock(memory.clone(), Arc::new(|| 1));

        let result = adapter.materialize(&request(true, Some(0))).await.unwrap();

        assert!(result.blocks.is_empty());
        assert!(memory.queries.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn disabled_result_is_empty_but_other_modes_are_errors() {
        let disabled = MemoryRetrieveAdapter::with_clock(
            Arc::new(FakeMemory::new(MemoryRetrievalMode::Disabled, vec![])),
            Arc::new(|| 1),
        );
        assert!(disabled
            .materialize(&request(true, Some(300)))
            .await
            .unwrap()
            .blocks
            .is_empty());

        let explicit = MemoryRetrieveAdapter::with_clock(
            Arc::new(FakeMemory::new(MemoryRetrievalMode::ExplicitSearch, vec![])),
            Arc::new(|| 1),
        );
        let error = explicit
            .materialize(&request(true, Some(300)))
            .await
            .unwrap_err();
        assert!(error.contains("InjectionPriority"));
        assert!(error.contains("ExplicitSearch"));
    }
}
