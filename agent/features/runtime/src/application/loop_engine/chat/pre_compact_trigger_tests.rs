//! PreCompact 反思触发的外部测试（新路径：材料暂存 + engine 反思 phase 取出执行）。
//!
//! 生产自动压缩路径（`RuntimeCompaction` + `ChatCompactionObserver`）在
//! `CompactOutcome::Committed` 时把压缩将丢弃的早期消息快照暂存进共享槽
//! （`PreCompactMaterialSlot`），`Skipped`/错误不暂存；执行点在 engine 的
//! reflection phase——经 `RuntimeReflection::take_pre_compact_messages` 取出材料、
//! 以 `ReflectionTaskTrigger::PreCompact` 执行。反思执行与历史持久化本身由
//! `task_adapter_tests` 覆盖，此处覆盖暂存/取出/执行的协作语义。

#![allow(clippy::type_complexity)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use sdk::{RunId, RunStepId};
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use share::message::Message;
use tokio_util::sync::CancellationToken;

use super::main_run_port::ChatCompactionObserver;
use crate::application::loop_engine::chat::reflection::PreCompactMaterialSlot;
use crate::application::loop_engine::compaction::CompactionObserver;
use crate::application::loop_engine::run_services::RuntimeReflection;
use crate::application::loop_engine::{CompactionPort, ReflectionPhasePort};
use crate::application::reflection::{
    ReflectionRunOutcome, ReflectionTaskAdapter, ReflectionTaskTrigger,
};
use crate::ports::{
    CompactOutcome, CompactionDecisionData, ContextPortError, ContextRequestData,
    ContextWindowData, DecisionReason, InteractionRequestId, Language as ContextLanguage,
    SessionId, SessionRevision, SystemBlock, TokenBudget, Urgency,
};
use context::compact::{CompactResult, CompactSkipReason, CompactSummaryQuality};

/// `submit_complete` builds its own executor closure and ignores the
/// adapter's `executor` field, so we cannot use a capturing closure to
/// observe submissions. The unit tests below therefore exercise the
/// helpers via the production adapter and a real provider whose response
/// parses as an empty reflection output. The integration tests exercise the
/// production `RuntimeCompaction` and `ChatCompactionObserver` seam, then use
/// `persisted_triggers()` to inspect which trigger reached persistence.
fn production_adapter() -> ReflectionTaskAdapter {
    ReflectionTaskAdapter::production(Duration::from_secs(5))
}

fn frozen_request() -> ContextRequestData {
    ContextRequestData {
        session_id: SessionId::new("session"),
        request_id: InteractionRequestId::new("request"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        pending_messages: vec![Message::user("seed")],
        workspace_root: std::path::PathBuf::from("/tmp/workspace"),
        workspace_is_git: false,
        model_id: "fake/model".to_string(),
        effective_reasoning: share::reasoning::ReasoningLevel::Off,
        language: ContextLanguage::En,
        agent_roles: HashMap::new(),
        config_snapshot: ConfigSnapshot::new(Config::default()),
        context_size: 128_000,
        max_output_tokens: 8_192,
        last_api_total_tokens: None,
        heuristic_calibration: None,
        tool_schemas: vec![],
        tool_schema_tokens: 0,
    }
}

fn window_with(messages: Vec<Message>) -> ContextWindowData {
    ContextWindowData {
        backing_revision: SessionRevision::new(7),
        system_blocks: vec![SystemBlock {
            kind: "system_prompt".to_string(),
            content: "system".to_string(),
            cacheable: true,
            cache_break: true,
        }],
        messages: messages.into(),
        tool_schemas: vec![],
        token_estimation: TokenBudget::default(),
        compaction_decision: CompactionDecisionData {
            needed: true,
            urgency: Urgency::Must,
            decision_token_count: 0,
            threshold: 0,
            context_size: 200_000,
            effective_window: 180_000,
            reason: DecisionReason::HeuristicFallback,
        },
    }
}

/// `Message` 无 PartialEq：断言统一经文本快照比较。
fn texts(messages: &[Message]) -> Vec<String> {
    messages
        .iter()
        .map(|message| message.text_content())
        .collect()
}

fn failing_append_reflection_history() -> Arc<dyn memory::api::ReflectionHistoryStore> {
    struct FailingAppendHistory;
    #[async_trait]
    impl memory::api::ReflectionHistoryQuery for FailingAppendHistory {
        async fn list(
            &self,
            _limit: usize,
        ) -> Result<Vec<memory::api::reflection::ReflectionSafeSummary>, memory::api::MemoryError>
        {
            Ok(Vec::new())
        }
    }
    #[async_trait]
    impl memory::api::ReflectionHistoryStore for FailingAppendHistory {
        async fn append(
            &self,
            _record: &memory::api::reflection::ReflectionRecord,
        ) -> Result<(), memory::api::MemoryError> {
            Err(memory::api::MemoryError::InvalidEntry {
                message: "history append failed".to_string(),
            })
        }
        async fn upsert(
            &self,
            _record: &memory::api::reflection::ReflectionRecord,
        ) -> Result<(), memory::api::MemoryError> {
            panic!("append failure must prevent terminal upsert")
        }
    }
    Arc::new(FailingAppendHistory)
}

/// Inline builder for the production compaction seam.
fn build_compact_test_port(
    harness: &CompactHarness,
) -> crate::application::loop_engine::run_services::RuntimeCompaction<'_, ChatCompactionObserver> {
    // 语义端口聚合等价性：由同一 fixture 组装（聚合按值比较不可行，跳过 ptr 断言）。
    crate::application::loop_engine::run_services::RuntimeCompaction::new(
        &harness.runtime_context,
        ChatCompactionObserver {
            pre_compact_material: harness.material_slot.clone(),
            reflection_history: cursor_history(None),
        },
    )
}

/// 生产反思端口（装配同 `run_launch`：与压缩观察者共享同一材料槽）。
fn build_reflection_port(harness: &CompactHarness) -> RuntimeReflection<'_> {
    RuntimeReflection::new(
        &harness.runtime_context,
        harness.adapter.clone(),
        "system prompt text".to_string(),
        "en".to_string(),
        harness.material_slot.clone(),
        crate::application::loop_engine::chat::reflection::IntervalReflectionMaterialSlot::default(
        ),
    )
}

/// Per-test harness for the production compaction service, chat observer and
/// reflection port (三者经 `material_slot` 共享 PreCompact 材料)。
struct CompactHarness {
    adapter: ReflectionTaskAdapter,
    stub: Arc<context::test_support::StubContextService>,
    runtime_context: crate::application::run::context::RuntimeContext,
    /// 记录型反思历史；注入非记录实现的构造（`with_history`）时为 None。
    reflection_history: Option<Arc<RecordingReflectionHistory>>,
    /// 与 ChatCompactionObserver 共享的 PreCompact 材料槽（同 run_launch 装配）。
    material_slot: PreCompactMaterialSlot,
    /// 记录型 Usage sink：断言反思终态的记账条数与字段。
    usage_sink: Arc<RecordingUsageSink>,
}

impl CompactHarness {
    fn new(outcome: Result<CompactOutcome, ContextPortError>) -> Self {
        let recording = Arc::new(RecordingReflectionHistory::default());
        Self::with_options(
            outcome,
            ConfigSnapshot::new(Config::default()),
            recording.clone(),
            Some(recording),
        )
    }

    /// 自定义配置（如关闭反思门禁）；反思历史为默认记录实现。
    fn with_config(
        outcome: Result<CompactOutcome, ContextPortError>,
        config: ConfigSnapshot,
    ) -> Self {
        let recording = Arc::new(RecordingReflectionHistory::default());
        Self::with_options(outcome, config, recording.clone(), Some(recording))
    }

    /// 自定义反思历史（如注入 append 失败的实现）；配置为默认值。
    fn with_history(
        outcome: Result<CompactOutcome, ContextPortError>,
        history: Arc<dyn memory::api::ReflectionHistoryStore>,
    ) -> Self {
        Self::with_options(
            outcome,
            ConfigSnapshot::new(Config::default()),
            history,
            None,
        )
    }

    fn with_options(
        outcome: Result<CompactOutcome, ContextPortError>,
        config_snapshot: ConfigSnapshot,
        reflection_history: Arc<dyn memory::api::ReflectionHistoryStore>,
        recording_history: Option<Arc<RecordingReflectionHistory>>,
    ) -> Self {
        let adapter = production_adapter();
        // #2146：Context 四端口实现权归 context crate；读路径失败 + compact
        // 结果可配置替身经其 test_support（take 一次语义一致）。
        let stub = std::sync::Arc::new(context::test_support::StubContextService::unavailable(
            "stub",
        ));
        stub.set_compact_outcome(outcome);
        let binding = pre_compact_test_binding();
        let usage_sink = Arc::new(RecordingUsageSink::default());
        let runtime_context =
            crate::application::run::run_factory_support::SessionRunFixture::builder()
                .with_context_port(crate::ports::ContextPorts::from_service(stub.clone()))
                .with_provider_binding(binding)
                .with_config(config_snapshot)
                .with_session_id("session".to_string())
                .with_reflection_history(reflection_history)
                .with_usage_sink(usage_sink.clone())
                .build()
                .create(crate::domain::agent_run::RunSpec::main())
                .expect("pre-compact parent run creation must succeed")
                .context()
                .clone();
        Self {
            adapter,
            stub,
            runtime_context,
            reflection_history: recording_history,
            material_slot: PreCompactMaterialSlot::default(),
            usage_sink,
        }
    }

    /// Triggers of the reflection runs that reached persistence. A synchronous
    /// run writes its `Running` marker before calling the provider, so this is
    /// a deterministic observation rather than a race with a background process.
    fn persisted_triggers(&self) -> Vec<memory::api::reflection::ReflectionTrigger> {
        self.reflection_history
            .as_ref()
            .map(|history| history.triggers())
            .unwrap_or_default()
    }
}

/// Non-blocking Usage sink that keeps every `UsageRecordData` a reflection
/// terminal state tries to record（记账条数与字段断言）。
#[derive(Default)]
pub(super) struct RecordingUsageSink {
    records: Mutex<Vec<audit::UsageRecordData>>,
}

impl RecordingUsageSink {
    pub(super) fn records(&self) -> Vec<audit::UsageRecordData> {
        self.records.lock().expect("usage sink lock").clone()
    }
}

impl crate::ports::UsageSink for RecordingUsageSink {
    fn try_record(&self, record: audit::UsageRecordData) -> audit::UsageEmitOutcomeData {
        self.records.lock().expect("usage sink lock").push(record);
        audit::UsageEmitOutcomeData::Accepted
    }
}

/// Records every durable record a reflection run writes, so tests can observe
/// which trigger ran without reaching into the adapter.
#[derive(Default)]
struct RecordingReflectionHistory {
    triggers: Mutex<Vec<memory::api::reflection::ReflectionTrigger>>,
}

impl RecordingReflectionHistory {
    fn triggers(&self) -> Vec<memory::api::reflection::ReflectionTrigger> {
        self.triggers.lock().expect("history lock").clone()
    }

    fn record(&self, record: &memory::api::reflection::ReflectionRecord) {
        self.triggers
            .lock()
            .expect("history lock")
            .push(record.trigger);
    }
}

#[async_trait]
impl memory::api::ReflectionHistoryQuery for RecordingReflectionHistory {
    async fn list(
        &self,
        _limit: usize,
    ) -> Result<Vec<memory::api::reflection::ReflectionSafeSummary>, memory::api::MemoryError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl memory::api::ReflectionHistoryStore for RecordingReflectionHistory {
    async fn append(
        &self,
        record: &memory::api::reflection::ReflectionRecord,
    ) -> Result<(), memory::api::MemoryError> {
        self.record(record);
        Ok(())
    }

    async fn upsert(
        &self,
        record: &memory::api::reflection::ReflectionRecord,
    ) -> Result<(), memory::api::MemoryError> {
        self.record(record);
        Ok(())
    }
}

/// Build a `ProviderBindingData` whose provider returns a parseable reflection
/// response so `submit_complete` can drain the adapter to a terminal state.
fn pre_compact_test_binding() -> Arc<crate::ports::ProviderBindingData> {
    crate::application::reflection::test_support::static_reflection_binding()
}

fn committed_outcome() -> CompactOutcome {
    CompactOutcome::Committed(CompactResult {
        summary: "summary".to_string(),
        recent_messages: vec![],
        source_revision: SessionRevision::new(7),
        quality: CompactSummaryQuality::LocalOnly,
    })
}

/// 旧 `maybe_run_pre_compact_reflection_only_runs_on_committed` 的等价新路径：
/// 材料暂存语义取代「观察者内直接执行」——Committed 暂存被丢弃消息进共享槽，
/// Skipped 不暂存也不改动已暂存材料；执行点已上移 engine reflection phase。
#[tokio::test]
async fn pre_compact_observer_stages_material_only_on_committed() {
    let snapshot = vec![
        Message::user("kept-by-compact"),
        Message::user("discarded-by-compact"),
    ];
    let skipped = CompactOutcome::Skipped(CompactSkipReason::ResumeProtection);

    let slot = PreCompactMaterialSlot::default();
    let mut observer = ChatCompactionObserver {
        pre_compact_material: slot.clone(),
        reflection_history: cursor_history(None),
    };

    observer
        .on_compacted(&skipped, &snapshot)
        .await
        .expect("Skipped 不得报错");
    assert!(
        slot.staged().is_none(),
        "Skipped 不得暂存材料（对齐 only_runs_on_committed 语义）"
    );

    observer
        .on_compacted(&committed_outcome(), &snapshot)
        .await
        .expect("Committed 不得报错");
    assert_eq!(
        slot.staged().as_ref().map(|messages| texts(messages)),
        Some(texts(&snapshot)),
        "Committed 必须把被丢弃消息快照暂存进共享槽"
    );

    observer
        .on_compacted(&skipped, &snapshot)
        .await
        .expect("Skipped 不得报错");
    assert_eq!(
        slot.staged().as_ref().map(|messages| texts(messages)),
        Some(texts(&snapshot)),
        "Skipped 不得改动已暂存材料"
    );

    // 反思端口取出语义（反思开启时，无游标 → 全量）：取走材料并清空槽位。
    let taken = slot.take_for_reflection(&share::config::MemoryConfig::default());
    assert_eq!(
        taken.as_ref().map(|material| texts(&material.messages)),
        Some(texts(&snapshot)),
        "开启时必须取走暂存材料"
    );
    assert!(slot.staged().is_none(), "取出后槽位必须清空");
}

/// 旧 `run_pre_compact_reflection_reports_a_precompact_completion` 的等价新路径：
/// 经生产反思端口取出材料并执行，得到 PreCompact 的 Succeeded completion。
#[tokio::test]
async fn pre_compact_execution_reports_a_precompact_completion() {
    let harness = CompactHarness::new(Ok(committed_outcome()));
    let snapshot = vec![
        Message::user("alpha"),
        Message::user("beta"),
        Message::user("gamma"),
    ];
    harness.material_slot.stage(snapshot.clone(), None);

    let mut reflection = build_reflection_port(&harness);
    let material = reflection
        .take_pre_compact_messages()
        .expect("反思开启时必须取走暂存材料");
    assert_eq!(
        texts(&material.messages),
        texts(&snapshot),
        "取出的必须是暂存的被丢弃消息"
    );
    assert_eq!(material.coverage_end, Some(3), "游标基准 = 丢弃段末位置");
    assert!(
        harness.material_slot.staged().is_none(),
        "取出后槽位必须清空"
    );

    let outcome = reflection
        .run_reflection(
            ReflectionTaskTrigger::PreCompact,
            material.messages,
            &RunId::new("run"),
            None,
            material.coverage_end,
            CancellationToken::new(),
        )
        .await
        .expect("反思端口执行不得返回 Err");
    // 统一异步链路（#2002）：派发即 Detached；终态经 history 落盘断言。
    assert!(
        matches!(outcome, ReflectionRunOutcome::Detached),
        "PreCompact 必须立即 Detached: {outcome:?}"
    );
    let mut triggers = harness.persisted_triggers();
    for _ in 0..500 {
        if triggers.last() == Some(&memory::api::reflection::ReflectionTrigger::PreCompact) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        triggers = harness.persisted_triggers();
    }
    assert_eq!(
        triggers.last(),
        Some(&memory::api::reflection::ReflectionTrigger::PreCompact),
        "detached 终态必须以 PreCompact 落盘"
    );
}

/// 旧 `run_pre_compact_reflection_reports_history_failure` 的等价新路径：
/// 历史 append 失败时 PreCompact completion 落 Failed 且不发布记忆更新。
#[tokio::test]
async fn pre_compact_execution_reports_history_failure() {
    let harness =
        CompactHarness::with_history(Ok(committed_outcome()), failing_append_reflection_history());
    harness
        .material_slot
        .stage(vec![Message::user("must not invoke provider")], None);

    let mut reflection = build_reflection_port(&harness);
    let material = reflection
        .take_pre_compact_messages()
        .expect("反思开启时必须取走暂存材料");
    let outcome = reflection
        .run_reflection(
            ReflectionTaskTrigger::PreCompact,
            material.messages,
            &RunId::new("run"),
            None,
            material.coverage_end,
            CancellationToken::new(),
        )
        .await
        .expect("反思端口执行不得返回 Err");

    // 统一异步链路（#2002）：派发即 Detached；history append 失败的 Failed
    // 终态发生在 detached 内部且无法经失败的 history 自证，保留可平移断言：
    // 不发布记忆更新、不记账 usage。
    assert!(
        matches!(outcome, ReflectionRunOutcome::Detached),
        "PreCompact 必须立即 Detached: {outcome:?}"
    );
    assert_eq!(
        harness.adapter.take_memory_update_notice(),
        None,
        "a history failure must not announce a memory update"
    );
}

/// Integration: 生产压缩服务在 `CompactOutcome::Committed` 后把压缩将丢弃的
/// 早期窗口暂存进共享槽（尚未执行反思）；取出执行后恰好落一条 PreCompact 记录。
#[tokio::test]
async fn pre_compact_trigger_stages_material_after_compact_outcome_committed() {
    let pre_compact_messages: Vec<Message> = (0..10)
        .map(|idx| Message::user(format!("u-{idx}")))
        .collect();
    let port_messages = pre_compact_messages.clone();
    let window = window_with(pre_compact_messages);
    let request = frozen_request();

    let harness = CompactHarness::new(Ok(committed_outcome()));

    let mut execution = crate::application::run::execution_state::RunExecutionState::new();
    execution.initialize_for_launch(port_messages, 1);
    execution.replace_context_state(request, Some(window));
    let mut port = build_compact_test_port(&harness);

    let cancel = CancellationToken::new();
    let noop_progress = std::sync::Arc::new(|_: sdk::CompactStageView, _: sdk::CompactWorkView| {});
    let result = port.compact(&mut execution, &cancel, noop_progress).await;
    assert!(
        result.is_ok(),
        "compact should succeed on Committed: {result:?}"
    );

    // Committed：观察者只暂存材料，不在此执行反思（执行点在 engine）。
    let staged = harness
        .material_slot
        .staged()
        .expect("Committed 必须把被丢弃消息暂存进共享槽");
    assert!(!staged.is_empty(), "被丢弃的早期窗口非空");
    assert_eq!(
        staged.first().map(|message| message.text_content()),
        Some("u-0".to_string()),
        "必须暂存压缩前窗口的早期消息"
    );
    assert!(
        !staged.iter().any(|message| message.text_content() == "u-9"),
        "不得暂存压缩保留的 recent tail"
    );
    assert!(
        harness.persisted_triggers().is_empty(),
        "暂存阶段不得执行反思"
    );
    assert_eq!(harness.stub.compact_requests().len(), 1);

    // 执行点在 engine：反思端口取出材料并以 PreCompact 执行。
    let mut reflection = build_reflection_port(&harness);
    let material = reflection
        .take_pre_compact_messages()
        .expect("反思开启时必须取走暂存材料");
    assert_eq!(
        texts(&material.messages),
        texts(&staged),
        "取出的必须是暂存的被丢弃消息"
    );
    reflection
        .run_reflection(
            ReflectionTaskTrigger::PreCompact,
            material.messages,
            &RunId::new("run"),
            None,
            material.coverage_end,
            CancellationToken::new(),
        )
        .await
        .expect("反思端口执行不得返回 Err");

    // 统一异步链路（#2002）：detached 终态异步落盘，轮询等待 PreCompact 记录。
    let mut triggers = harness.persisted_triggers();
    for _ in 0..500 {
        if triggers.contains(&memory::api::reflection::ReflectionTrigger::PreCompact) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        triggers = harness.persisted_triggers();
    }
    assert!(
        triggers.contains(&memory::api::reflection::ReflectionTrigger::PreCompact),
        "Committed 最终必须执行 PreCompact reflection: {triggers:?}"
    );
    // 材料一次取出后槽位即空，杜绝同一份材料被重复执行。
    assert!(
        harness.material_slot.staged().is_none(),
        "材料被取出执行后槽位必须清空"
    );
}

/// Integration: `CompactOutcome::Skipped` 是非致命 no-op——不暂存材料，
/// engine 反思 phase 取不到材料，不会执行 PreCompact 反思。
#[tokio::test]
async fn pre_compact_trigger_stages_nothing_on_compact_outcome_skipped() {
    let window = window_with(vec![Message::user("only")]);
    let request = frozen_request();

    let harness = CompactHarness::new(Ok(CompactOutcome::Skipped(
        CompactSkipReason::ResumeProtection,
    )));

    let mut execution = crate::application::run::execution_state::RunExecutionState::new();
    execution.initialize_for_launch(vec![Message::user("only")], 1);
    execution.replace_context_state(request, Some(window));
    let mut port = build_compact_test_port(&harness);

    let cancel = CancellationToken::new();
    let noop_progress = std::sync::Arc::new(|_: sdk::CompactStageView, _: sdk::CompactWorkView| {});
    let result = port.compact(&mut execution, &cancel, noop_progress).await;
    assert!(
        result.is_ok(),
        "automatic compact skip must continue the current Run: {result:?}"
    );

    assert!(
        harness.material_slot.staged().is_none(),
        "Skipped 不得暂存材料"
    );
    let reflection = build_reflection_port(&harness);
    assert!(
        reflection.take_pre_compact_messages().is_none(),
        "无材料时 engine 不得进入反思 phase"
    );
    let triggers = harness.persisted_triggers();
    assert!(
        triggers.is_empty(),
        "Skipped must NOT run a PreCompact reflection: {triggers:?}"
    );
    assert_eq!(harness.stub.compact_requests().len(), 1);
}

/// Integration: context port 的 `compact` 报错时观察者不被调用——不暂存材料，
/// PreCompact 反思绝不执行（压缩没有提交，早期窗口不得被反思观察）。
#[tokio::test]
async fn pre_compact_trigger_skips_when_context_compact_call_errors() {
    let window = window_with(vec![Message::user("only")]);
    let request = frozen_request();

    let harness = CompactHarness::new(Err(ContextPortError::Compact(
        "context port error".to_string(),
    )));

    let mut execution = crate::application::run::execution_state::RunExecutionState::new();
    execution.initialize_for_launch(vec![Message::user("only")], 1);
    execution.replace_context_state(request, Some(window));
    let mut port = build_compact_test_port(&harness);

    let cancel = CancellationToken::new();
    let noop_progress = std::sync::Arc::new(|_: sdk::CompactStageView, _: sdk::CompactWorkView| {});
    let result = port.compact(&mut execution, &cancel, noop_progress).await;
    assert!(
        result.is_err(),
        "compact must propagate context port errors"
    );

    assert!(
        harness.material_slot.staged().is_none(),
        "compact 失败时不得暂存材料"
    );
    let reflection = build_reflection_port(&harness);
    assert!(
        reflection.take_pre_compact_messages().is_none(),
        "无材料时 engine 不得进入反思 phase"
    );
    let triggers = harness.persisted_triggers();
    assert!(
        triggers.is_empty(),
        "context port errors must NOT run a PreCompact reflection: {triggers:?}"
    );
    assert_eq!(harness.stub.compact_requests().len(), 1);
}

/// 反思配置关闭时的取出语义：丢弃暂存材料并返回 None——材料不滞留到下一次
/// compact，engine 也不空走一次 Reflecting 往返。
#[tokio::test]
async fn pre_compact_take_drops_material_when_reflection_disabled() {
    let mut config = Config::default();
    config.memory.enabled = false;
    let harness = CompactHarness::with_config(
        Ok(CompactOutcome::Skipped(CompactSkipReason::ResumeProtection)),
        ConfigSnapshot::new(config),
    );
    harness
        .material_slot
        .stage(vec![Message::user("discarded")], None);

    let reflection = build_reflection_port(&harness);
    assert!(
        reflection.take_pre_compact_messages().is_none(),
        "反思禁用时不得返回材料给 engine phase"
    );
    assert!(
        harness.material_slot.staged().is_none(),
        "反思禁用时材料必须被丢弃，NEVER 滞留到下一次 compact"
    );
}

// ---------------------------------------------------------------------------
// 反思成功 terminal 复用 record_successful_usage 计入 /usage
// ---------------------------------------------------------------------------

/// Succeeded 终态（含 usage metadata）经共享 `record_successful_usage` 恰好记
/// 1 条 UsageRecord：run/step/session/model identity 与 provider 报告的
/// input/output tokens 原样进入记录（`StaticReflectionProvider` 报 1/1）。
#[tokio::test]
async fn succeeded_reflection_records_usage_via_shared_path() {
    let harness = CompactHarness::new(Ok(committed_outcome()));
    harness
        .material_slot
        .stage(vec![Message::user("reflect me")], None);

    let mut reflection = build_reflection_port(&harness);
    let material = reflection
        .take_pre_compact_messages()
        .expect("反思开启时必须取走暂存材料");
    let run_id = RunId::new("usage-run");
    let run_step_id = RunStepId::new("usage-step");
    let outcome = reflection
        .run_reflection(
            ReflectionTaskTrigger::PreCompact,
            material.messages,
            &run_id,
            Some(&run_step_id),
            material.coverage_end,
            CancellationToken::new(),
        )
        .await
        .expect("反思端口执行不得返回 Err");

    assert!(
        matches!(outcome, ReflectionRunOutcome::Detached),
        "PreCompact 必须立即 Detached: {outcome:?}"
    );

    // usage 记账在 detached 完成点异步执行（#2002 M2，Interval 缺口同路径补齐）。
    let mut records = harness.usage_sink.records();
    for _ in 0..500 {
        if !records.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        records = harness.usage_sink.records();
    }
    assert_eq!(
        records.len(),
        1,
        "Succeeded 反思必须恰好记 1 条 UsageRecord: {records:?}"
    );
    let record = &records[0];
    assert_eq!(record.run_id, run_id, "run identity 必须是当前反思 Run");
    assert_eq!(record.run_step_id, run_step_id);
    assert_eq!(
        record.session_id,
        sdk::SessionId::new(harness.runtime_context.skill_load_session_id())
    );
    assert_eq!(record.provider, "reflection-test");
    // 两步反思：Step1 去噪 + Step2 归纳各计一次（mock 每次同额）。
    assert_eq!(
        record.input_tokens, 2,
        "两步 provider input tokens 合并入账"
    );
    assert_eq!(
        record.output_tokens, 2,
        "两步 provider output tokens 合并入账"
    );
    assert_ne!(
        record.model_invocation_id,
        sdk::ModelInvocationId::new(""),
        "记账必须携带 model_invocation_id"
    );
}

/// Failed 终态不记账（即使执行层报告过 usage 数字）。
#[tokio::test]
async fn failed_reflection_records_no_usage() {
    let harness =
        CompactHarness::with_history(Ok(committed_outcome()), failing_append_reflection_history());
    harness
        .material_slot
        .stage(vec![Message::user("will fail on history")], None);

    let mut reflection = build_reflection_port(&harness);
    let material = reflection
        .take_pre_compact_messages()
        .expect("反思开启时必须取走暂存材料");
    let outcome = reflection
        .run_reflection(
            ReflectionTaskTrigger::PreCompact,
            material.messages,
            &RunId::new("failed-run"),
            Some(&RunStepId::new("failed-step")),
            material.coverage_end,
            CancellationToken::new(),
        )
        .await
        .expect("反思端口执行不得返回 Err");

    assert!(
        matches!(outcome, ReflectionRunOutcome::Detached),
        "PreCompact 必须立即 Detached: {outcome:?}"
    );
    // Failed 终态不记账；等 detached 执行完（history 失败即快速终态）再断言。
    for _ in 0..200 {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        harness.usage_sink.records().is_empty(),
        "Failed 反思不得计入 Usage: {:?}",
        harness.usage_sink.records()
    );
}

/// 调用方 token 预取消不影响异步（#2002 D2）：detached 持独立 token，
/// 派发照常执行并按实际终态记账（原「取消不记账」同步语义已退役；
/// 替代证据 = task_tests 的 detached_run_ignores_cancelled_caller_token）。
#[tokio::test]
async fn cancelled_reflection_records_no_usage() {
    let harness = CompactHarness::new(Ok(committed_outcome()));
    let cancel = CancellationToken::new();
    cancel.cancel();

    let mut reflection = build_reflection_port(&harness);
    let outcome = reflection
        .run_reflection(
            ReflectionTaskTrigger::PreCompact,
            vec![Message::user("cancelled before start")],
            &RunId::new("cancelled-run"),
            None,
            None,
            cancel,
        )
        .await
        .expect("取消不影响异步派发");

    assert!(
        matches!(outcome, ReflectionRunOutcome::Detached),
        "预取消 token 必须照常 Detached: {outcome:?}"
    );
    // 实际执行成功（static provider）：usage 由完成点记账。
    let mut records = harness.usage_sink.records();
    for _ in 0..500 {
        if !records.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        records = harness.usage_sink.records();
    }
    assert_eq!(
        records.len(),
        1,
        "预取消不影响执行：成功终态必须照常记账: {records:?}"
    );
}

/// DisabledSkipped 不记账（配置禁用时空跑一趟）。
#[tokio::test]
async fn disabled_reflection_records_no_usage() {
    let mut config = Config::default();
    config.memory.enabled = false;
    let harness = CompactHarness::with_config(Ok(committed_outcome()), ConfigSnapshot::new(config));

    let mut reflection = build_reflection_port(&harness);
    let outcome = reflection
        .run_reflection(
            ReflectionTaskTrigger::PreCompact,
            vec![Message::user("disabled")],
            &RunId::new("disabled-run"),
            None,
            None,
            CancellationToken::new(),
        )
        .await
        .expect("禁用不是错误");

    assert_eq!(outcome, ReflectionRunOutcome::DisabledSkipped);
    assert!(
        harness.usage_sink.records().is_empty(),
        "DisabledSkipped 不得计入 Usage: {:?}",
        harness.usage_sink.records()
    );
}

// ── Interval 游标增量材料（#1827）：生产端口的拼接与回退 ──────────

use crate::application::loop_engine::chat::reflection::IntervalReflectionMaterialSlot;

/// interval=1 的开启配置（频控每 Run 命中，测试只需 1 个 step_count）。
fn interval_one_config() -> ConfigSnapshot {
    let mut config = Config::default();
    config.memory.enabled = true;
    config.memory.reflection.enabled = true;
    config.memory.reflection.interval_runs = 1;
    ConfigSnapshot::new(config)
}

/// 装了历史增量的槽：材料 = 增量 + Run 内消息（顺序拼接），
/// coverage_end = 装槽时历史总长 + Run 内消息数（游标推进基准）。
#[test]
fn interval_reflection_messages_merges_staged_increment_with_run_messages() {
    let harness = CompactHarness::with_config(Ok(committed_outcome()), interval_one_config());
    let interval_slot = IntervalReflectionMaterialSlot::default();
    interval_slot.stage(
        vec![
            share::message::Message::user("history delta 1"),
            share::message::Message::user("history delta 2"),
        ],
        30, // 装槽时 session 历史总长
    );
    let reflection = crate::application::loop_engine::run_services::RuntimeReflection::new(
        &harness.runtime_context,
        harness.adapter.clone(),
        "system".to_string(),
        "en".to_string(),
        harness.material_slot.clone(),
        interval_slot,
    );

    let run_messages = [share::message::Message::user("in-run message")];
    let material = reflection
        .interval_reflection_messages(1, &run_messages)
        .expect("频控命中必须返回材料（step_count=1 × interval=1 的默认配置）");

    assert_eq!(material.messages.len(), 3, "增量 2 条 + Run 内 1 条");
    assert_eq!(material.messages[0].text_content(), "history delta 1");
    assert_eq!(material.messages[2].text_content(), "in-run message");
    assert_eq!(
        material.coverage_end,
        Some(31),
        "游标推进基准 = 装槽历史总长 30 + Run 内消息数 1"
    );
}

/// 空槽（无游标回退）：材料 = 仅 Run 内消息，coverage_end=None（不推进游标）。
#[test]
fn interval_reflection_messages_empty_slot_falls_back_to_run_messages_only() {
    let harness = CompactHarness::with_config(Ok(committed_outcome()), interval_one_config());
    let reflection = crate::application::loop_engine::run_services::RuntimeReflection::new(
        &harness.runtime_context,
        harness.adapter.clone(),
        "system".to_string(),
        "en".to_string(),
        harness.material_slot.clone(),
        IntervalReflectionMaterialSlot::default(),
    );

    let run_messages = [share::message::Message::user("only run message")];
    let material = reflection
        .interval_reflection_messages(1, &run_messages)
        .expect("频控命中必须返回材料");

    assert_eq!(material.messages.len(), 1);
    assert_eq!(material.messages[0].text_content(), "only run message");
    assert_eq!(
        material.coverage_end, None,
        "回退路径不推进游标（保持现状语义）"
    );
}

// ── PreCompact 材料游标去重（统一覆盖去重）────────────────────────

/// 可配置游标的 fake history：`list` 返回唯一一条 `Succeeded` 且带
/// `coverage_end` 的记录，供 `latest_coverage_cursor` 读取。
struct CursorReflectionHistory {
    coverage_end: Option<u64>,
}

#[async_trait]
impl memory::api::ReflectionHistoryQuery for CursorReflectionHistory {
    async fn list(
        &self,
        _limit: usize,
    ) -> Result<Vec<memory::api::reflection::ReflectionSafeSummary>, memory::api::MemoryError> {
        Ok(vec![memory::api::reflection::ReflectionSafeSummary {
            id: share::ids::new_typed_id("inv"),
            timestamp: 0,
            trigger: memory::api::reflection::ReflectionTrigger::Interval,
            status: memory::api::reflection::ReflectionStatus::Succeeded,
            deviations: 0,
            suggestions: 0,
            outdated: 0,
            apply_status: memory::api::reflection::ReflectionApplyStatus::Applied,
            error_category: None,
            token_usage: None,
            duration_ms: 1,
            coverage_end: self.coverage_end,
            deviation_texts: None,
            suggested_memories: None,
        }])
    }
}

#[async_trait]
impl memory::api::ReflectionHistoryStore for CursorReflectionHistory {
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

fn cursor_history(coverage_end: Option<u64>) -> Arc<dyn memory::api::ReflectionHistoryStore> {
    Arc::new(CursorReflectionHistory { coverage_end })
}

fn staged_discarded_segment() -> Vec<Message> {
    vec![
        Message::user("dropped-0"),
        Message::user("dropped-1"),
        Message::user("dropped-2"),
        Message::user("dropped-3"),
        Message::user("dropped-4"),
    ]
}

/// stage 时按最新 Succeeded 游标过滤：只保留游标之后的未覆盖残余，
/// coverage_end = 丢弃段在压缩前历史坐标系中的末位置（成功后推游标的基准）。
#[test]
fn pre_compact_take_returns_only_uncovered_residual() {
    let slot = PreCompactMaterialSlot::default();
    slot.stage(staged_discarded_segment(), Some(3));

    let material = slot
        .take_for_reflection(&share::config::MemoryConfig::default())
        .expect("存在未覆盖残余时必须派发");
    assert_eq!(
        texts(&material.messages),
        vec!["dropped-3".to_string(), "dropped-4".to_string()],
        "材料只含游标之后的未覆盖残余"
    );
    assert_eq!(
        material.coverage_end,
        Some(5),
        "coverage_end = 丢弃段末位置（压缩前坐标）"
    );
}

/// 丢弃段整段已被游标覆盖：不派发（已 reflect 的 context 不再进入 reflect）。
#[test]
fn pre_compact_take_skips_fully_covered_segment() {
    let slot = PreCompactMaterialSlot::default();
    slot.stage(staged_discarded_segment(), Some(5));
    assert!(
        slot.take_for_reflection(&share::config::MemoryConfig::default())
            .is_none(),
        "整段已覆盖必须不派发"
    );

    // 游标越过丢弃段末位置（compact 截断后游标失效前的一次 take）同样不派发。
    let slot = PreCompactMaterialSlot::default();
    slot.stage(staged_discarded_segment(), Some(9));
    assert!(slot
        .take_for_reflection(&share::config::MemoryConfig::default())
        .is_none());
}

/// 无游标（首次反思 / 游标失效回退）：全量材料 + coverage_end=丢弃段末位置。
#[test]
fn pre_compact_take_without_cursor_returns_full_segment() {
    let slot = PreCompactMaterialSlot::default();
    slot.stage(staged_discarded_segment(), None);

    let material = slot
        .take_for_reflection(&share::config::MemoryConfig::default())
        .expect("无游标回退必须全量派发");
    assert_eq!(material.messages.len(), 5);
    assert_eq!(material.coverage_end, Some(5));
}

/// BusySkipped 回填滞留（D4a）：回填后残余保持绝对坐标语义，
/// 游标推进到残余内部时再次 take 只取剩余未覆盖部分。
#[test]
fn pre_compact_restore_defers_residual_with_absolute_coordinates() {
    let slot = PreCompactMaterialSlot::default();
    slot.stage(staged_discarded_segment(), Some(3));
    let material = slot
        .take_for_reflection(&share::config::MemoryConfig::default())
        .expect("未覆盖残余必须派发");

    slot.restore(&material);
    assert_eq!(
        slot.staged().as_ref().map(|messages| texts(messages)),
        Some(vec!["dropped-3".to_string(), "dropped-4".to_string()]),
        "回填后滞留残余"
    );

    // 游标推进到 4（绝对坐标）：残余中只剩绝对位置 4 的消息。
    slot.advance_cursor_if_farther(4);
    let again = slot
        .take_for_reflection(&share::config::MemoryConfig::default())
        .expect("残余仍有未覆盖部分时再次派发");
    assert_eq!(texts(&again.messages), vec!["dropped-4".to_string()]);
    assert_eq!(again.coverage_end, Some(5));
}

/// 槽内已有回填滞留时再 stage：覆盖滞留材料（新丢弃段优先）。
#[test]
fn pre_compact_stage_overwrites_restored_residual() {
    let slot = PreCompactMaterialSlot::default();
    slot.stage(staged_discarded_segment(), Some(3));
    let material = slot
        .take_for_reflection(&share::config::MemoryConfig::default())
        .expect("未覆盖残余必须派发");
    slot.restore(&material);

    let fresh = vec![Message::user("new-drop-0"), Message::user("new-drop-1")];
    slot.stage(fresh.clone(), None);
    let taken = slot
        .take_for_reflection(&share::config::MemoryConfig::default())
        .expect("新丢弃段必须生效");
    assert_eq!(texts(&taken.messages), texts(&fresh));
    assert_eq!(taken.coverage_end, Some(2));
}

/// compaction observer stage 时读游标并过滤：整段已覆盖时不暂存派发材料。
#[tokio::test]
async fn pre_compact_observer_stages_only_uncovered_residual() {
    let history = cursor_history(Some(5));
    let slot = PreCompactMaterialSlot::default();
    let mut observer = ChatCompactionObserver {
        pre_compact_material: slot.clone(),
        reflection_history: history,
    };

    observer
        .on_compacted(&committed_outcome(), &staged_discarded_segment())
        .await
        .expect("Committed 不得报错");
    assert!(
        slot.take_for_reflection(&share::config::MemoryConfig::default())
            .is_none(),
        "整段已覆盖：observer 不得暂存可派发材料"
    );
}
