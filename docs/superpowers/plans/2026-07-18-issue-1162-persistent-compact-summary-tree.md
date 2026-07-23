# Persistent Compact Summary Tree Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用 7 个独立 PR 将同步、不可恢复的 Auto-compact map-reduce 迁移为持久化增量摘要树，实现 per-session 1 / global 5 调度、warm projection、完整 compact attempt token 总账和读取时 cost estimate。

**Architecture:** Context Management 独占摘要树、coverage、projection 和 compact attempt 聚合策略；Provider adapter 只生成结构化摘要并返回标准化 usage；Storage atomic dataset adapter 提供 sidecar 原子持久化；Composition 唯一装配进程级 scheduler，并可在 SummaryGenerator 外层把成功逻辑 invocation best-effort 投影到 Audit；Context 不持有 UsagePort / UsageSink。Runtime、SDK、TUI 只消费 Context-owned Published Language。每个 Issue 在独立 worktree 中从最新 `origin/release/v0.1.0` 开始，按 TDD 完成并开独立 PR。

**Tech Stack:** Rust、Tokio、Serde、Storage `AtomicDatasetPort`、Provider `ProviderCompletion`、GitHub native sub-issues/dependencies、Cargo workspace tests、architecture guards。

---

## 0. 文件结构与执行顺序

目标新增 / 修改职责如下：

```text
agent/features/context/src/
├── domain/compact/
│   ├── summary_tree.rs              # #1163 值对象、coverage、树与 projection 纯策略
│   └── summary_tree_tests.rs        # #1163 L1 不变量
├── ports/
│   ├── summary_generator.rs         # #1164 Provider-neutral 摘要端口
│   └── compact_index.rs             # #1165 Sidecar 端口
├── adapters/
│   ├── provider_summary_generator.rs       # #1164 Provider ACL
│   └── atomic_dataset_compact_index.rs     # #1165 Storage adapter
├── application/
│   ├── compact_scheduler.rs         # #1168 公平调度、permit、retry
│   └── summary_projection.rs        # #1166 增量调度与 warm activation
└── tests/
    └── persistent_compact_lifecycle.rs     # #1119 L4 生命周期

agent/composition/src/runtime.rs               # #1168 唯一 scheduler 装配
agent/features/runtime/src/application/main_loop/looping/
├── compact.rs                                 # #1166 ContextPort 接入
└── events.rs                                  # #1167 status/usage PL
agent/features/runtime/src/adapters/event_projection.rs
packages/sdk/src/chat_event.rs
apps/cli/src/tui/effect/session/processing/event_mapping.rs
apps/cli/src/tui/model/conversation/
├── compact_progress.rs
└── usage.rs                                   # #1167 相邻投影与展示状态
```

执行依赖：

```text
#1163
  ├─> #1164 ─┐
  └─> #1165 ─┴─> #1168 ─> #1166 ─> #1167 ─> #1119
```

每个 Task 完成后只提交其 Issue 范围并创建 `feature/bugfix → release/v0.1.0` PR；**NEVER** 把 7 个 Task 合成一个实现 PR。

### Task 1: #1163 摘要树领域模型与 coverage invariant

**Files:**

- Create: `agent/features/context/src/domain/compact/summary_tree.rs`
- Create: `agent/features/context/src/domain/compact/summary_tree_tests.rs`
- Modify: `agent/features/context/src/domain/compact.rs`
- Modify: `agent/features/context/src/domain.rs`
- Modify: `agent/features/context/src/lib.rs`
- Test: `agent/features/context/src/domain/compact/summary_tree_tests.rs`

- [ ] **Step 1: 在独立 worktree 写 coverage 与 projection 失败测试**

```rust
#[test]
fn projection_rejects_gap_between_adjacent_nodes() {
    let first = leaf(step(1), step(4));
    let second = leaf(step(5), step(8));

    let error = CompactProjection::select(
        generation(),
        vec![first, second],
        summary_budget(8_000),
        revision(7),
    )
    .expect_err("step 4..5 gap must be rejected");

    assert!(matches!(error, ProjectionError::CoverageGap { .. }));
}

#[test]
fn provider_or_model_change_does_not_change_shard_identity() {
    let source = normalized_source(step(1), step(4));
    let first = SummaryShardId::derive(&source, versions(1, 1, 1));
    let second = SummaryShardId::derive(&source, versions(1, 1, 1));

    assert_eq!(first, second);
}

#[test]
fn branch_requires_two_to_four_adjacent_same_level_children() {
    let error = SummaryShard::branch(
        generation(),
        vec![leaf(step(1), step(2)), leaf(step(3), step(4))],
        summary_document(),
        invocation_id(),
    )
    .expect_err("non-adjacent children must be rejected");

    assert!(matches!(error, SummaryTreeError::NonAdjacentChildren { .. }));
}
```

- [ ] **Step 2: 运行测试确认因模型不存在而失败**

Run:

```bash
cargo test -p context --lib domain::compact::summary_tree_tests
```

Expected: FAIL，错误包含 `cannot find type CompactProjection`、`SummaryShardId` 或 `SummaryShard`。

- [ ] **Step 3: 实现最小领域模型与纯策略**

```rust
pub struct CoverageRange {
    pub first: RunStepRef,
    pub end_exclusive: RunStepRef,
}

pub struct SummaryShard {
    pub id: SummaryShardId,
    pub generation: SummaryGeneration,
    pub coverage: CoverageRange,
    pub source_fingerprint: ContentFingerprint,
    pub kind: SummaryShardKind,
    pub document: SummaryDocument,
    pub invocation_id: ModelInvocationId,
}

impl SummaryShard {
    pub fn branch(
        generation: SummaryGeneration,
        children: Vec<SummaryShard>,
        document: SummaryDocument,
        invocation_id: ModelInvocationId,
    ) -> Result<Self, SummaryTreeError> {
        validate_branch_children(&children)?;
        let coverage = CoverageRange::join(
            children.first().expect("validated non-empty").coverage.clone(),
            children.last().expect("validated non-empty").coverage.clone(),
        )?;
        Ok(Self::new_branch(
            generation,
            coverage,
            children,
            document,
            invocation_id,
        ))
    }
}
```

实现必须同时覆盖：

- finalized-only candidate；
- recent 3 Run 保护区；
- Branch children `2..=4`；
- 连续、无重叠 projection；
- source evidence 合并；
- directive 后续修正保留旧来源；
- shard identity 只包含 fingerprint 与三项版本。

- [ ] **Step 4: 跑领域测试、Context crate 和格式门禁**

Run:

```bash
cargo test -p context --lib domain::compact
cargo test -p context --tests
cargo fmt --all -- --check
```

Expected: 全部 PASS，且没有 Provider、Runtime、Storage I/O 依赖进入 domain。

- [ ] **Step 5: 提交并创建 #1163 PR**

```bash
git add agent/features/context/src/domain agent/features/context/src/lib.rs
git commit -m "feat(context): #1163 建立持久化摘要树领域模型"
git pull --no-rebase origin release/v0.1.0
git push -u origin feat/1163-compact-summary-tree-domain
```

PR base 必须是 `release/v0.1.0`，Refs 写 `#1163`。

### Task 2: #1164 SummaryGenerator、规范化输入与结构化 schema

**Files:**

- Create: `agent/features/context/src/ports/summary_generator.rs`
- Create: `agent/features/context/src/adapters/provider_summary_generator.rs`
- Create: `agent/features/context/src/adapters/provider_summary_generator_tests.rs`
- Modify: `agent/features/context/src/ports.rs`
- Modify: `agent/features/context/src/adapters.rs`
- Modify: `agent/features/context/src/adapters/compact_summary.rs`
- Test: `agent/features/context/src/adapters/provider_summary_generator_tests.rs`

- [ ] **Step 1: 写规范化边界与 usage 透传失败测试**

```rust
#[tokio::test]
async fn normalization_keeps_tool_pair_in_one_step_and_skips_thinking() {
    let source = committed_step_with_user_tool_thinking_and_result();
    let request = SummaryRequest::leaf(source, versions(1, 1, 1));

    let normalized = normalize_summary_input(&request).expect("normalization");

    assert!(!normalized.text.contains("private chain of thought"));
    assert!(normalized.text.contains("Read"));
    assert!(normalized.text.contains("/workspace/src/lib.rs"));
    assert!(normalized.text.contains("[tool result truncated]"));
    assert_eq!(normalized.steps.len(), 1);
}

#[tokio::test]
async fn provider_completion_usage_is_returned_without_context_recalculation() {
    let provider = scripted_provider(completion_with_usage(100, 20, 80, 30, 150));
    let generator = ProviderSummaryGenerator::new(provider);

    let completion = generator
        .generate(&summary_request(), &CancellationToken::new())
        .await
        .expect("summary generation");

    assert_eq!(completion.usage.input_tokens, Some(100));
    assert_eq!(completion.usage.output_tokens, Some(20));
    assert_eq!(completion.usage.normalized_total_tokens, Some(150));
    assert_eq!(completion.usage.source, UsageSource::ProviderReported);
}
```

- [ ] **Step 2: 运行 adapter 测试确认失败**

Run:

```bash
cargo test -p context --lib adapters::provider_summary_generator_tests
```

Expected: FAIL，`ProviderSummaryGenerator`、`SummaryRequest` 或 `normalize_summary_input` 尚不存在。

- [ ] **Step 3: 实现端口、16K/24K 分块与 Provider ACL**

```rust
#[async_trait]
pub trait SummaryGenerator: Send + Sync {
    async fn generate(
        &self,
        request: &SummaryRequest,
        cancel: &CancellationToken,
    ) -> Result<SummaryCompletion, SummaryGenerationError>;
}

pub const LEAF_TARGET_TOKENS: usize = 16_000;
pub const LEAF_SOFT_LIMIT_TOKENS: usize = 24_000;

pub struct SummaryCompletion {
    pub document: SummaryDocument,
    pub usage: NormalizedInvocationUsage,
    pub provider: String,
    pub model: String,
}
```

迁移 `COMPACT_PROMPT` 为结构化 `SummaryDocument` schema；`ProviderCompletion.usage == None` 映射为 `UsageSource::Unknown`，**NEVER** 填零。

- [ ] **Step 4: 验证正常、ContextTooLong 与不可重试错误分类**

Run:

```bash
cargo test -p context --lib adapters::provider_summary_generator
cargo test -p context --tests
cargo test -p provider --lib
cargo fmt --all -- --check
```

Expected: 全部 PASS；相同超长请求不会被原样重发；Provider 私有 wire 类型不进入 Context domain。

- [ ] **Step 5: 提交并创建 #1164 PR**

```bash
git add agent/features/context
git commit -m "feat(context): #1164 建立结构化摘要生成端口"
git pull --no-rebase origin release/v0.1.0
git push -u origin feat/1164-summary-generator
```

### Task 3: #1165 CompactIndex Sidecar、恢复与 GC

**Files:**

- Create: `agent/features/context/src/ports/compact_index.rs`
- Create: `agent/features/context/src/adapters/atomic_dataset_compact_index.rs`
- Create: `agent/features/context/src/adapters/atomic_dataset_compact_index_tests.rs`
- Create: `agent/features/context/tests/compact_index_contract.rs`
- Modify: `agent/features/context/src/ports.rs`
- Modify: `agent/features/context/src/adapters.rs`
- Modify: `agent/features/context/Cargo.toml`
- Test: `agent/features/context/tests/compact_index_contract.rs`

- [ ] **Step 1: 写 checkpoint、CAS 与 orphan 收养失败测试**

```rust
#[tokio::test]
async fn committed_shard_and_usage_are_recovered_together() {
    let fixture = CompactIndexFixture::new();
    let receipt = fixture
        .index
        .commit_shard(commit_request(shard(), usage()))
        .await
        .expect("atomic checkpoint");

    let reopened = fixture.reopen().await.expect("reopen index");

    assert_eq!(reopened.manifest.revision, receipt.revision);
    assert_eq!(reopened.shards, vec![shard()]);
    assert_eq!(reopened.usage, vec![usage()]);
}

#[tokio::test]
async fn orphan_with_matching_source_is_adopted_without_generator_call() {
    let fixture = CompactIndexFixture::with_orphan(shard(), usage());
    let recovered = fixture.reopen().await.expect("recover orphan");

    assert_eq!(recovered.manifest.frontier, shard().coverage.end_exclusive);
    assert_eq!(fixture.generator_calls(), 0);
}

#[tokio::test]
async fn stale_manifest_revision_rejects_publish_without_partial_projection() {
    let fixture = CompactIndexFixture::new();
    let stale = fixture.manifest_revision();
    fixture.advance_manifest().await;

    let error = fixture
        .index
        .publish_projection(publish_request(stale))
        .await
        .expect_err("stale CAS must fail");

    assert!(matches!(error, CompactIndexError::RevisionConflict { .. }));
    assert!(fixture.active_projection().await.is_none());
}
```

- [ ] **Step 2: 运行 contract 测试确认失败**

Run:

```bash
cargo test -p context --test compact_index_contract
```

Expected: FAIL，因为 `CompactIndex` 和 `AtomicDatasetCompactIndex` 尚不存在。

- [ ] **Step 3: 用 Storage AtomicDatasetPort 实现 sidecar**

```rust
#[async_trait]
pub trait CompactIndex: Send + Sync {
    async fn load(&self, session: &SessionId) -> Result<CompactIndexSnapshot, CompactIndexError>;
    async fn commit_shard(
        &self,
        request: CommitShardRequest,
    ) -> Result<CommitShardReceipt, CompactIndexError>;
    async fn publish_projection(
        &self,
        request: PublishProjectionRequest,
    ) -> Result<PublishProjectionReceipt, CompactIndexError>;
    async fn collect_garbage(
        &self,
        request: CompactGcRequest,
    ) -> Result<CompactGcReceipt, CompactIndexError>;
}

pub struct AtomicDatasetCompactIndex {
    storage: Arc<dyn storage::AtomicDatasetPort>,
    root_namespace: storage::StorageNamespace,
}
```

每次 `commit_shard` 以 `SessionId + SummaryShardId` 生成 checkpoint
`DatasetKey`，用一次 `commit_atomic` 写入 `shard.json` 与
`success-attempt.json`；随后对独立 session manifest dataset 做 CAS。每个已
返回的失败 / 取消 attempt 必须在 retry 或返回调用方前，按
`SessionId + ModelInvocationId + attempt` 写独立 usage AtomicDataset。一个
Compact Job 的 retry 复用同一 `ModelInvocationId` 并递增 attempt ordinal。
**NEVER** 在 Context adapter 内实现裸 `fsync` / `rename`，也 **NEVER** 为
新增一个 shard 重写全部历史 shard。

- [ ] **Step 4: 跑 crash / recovery / GC 验证**

Run:

```bash
cargo test -p context --test compact_index_contract
cargo test -p context --lib adapters::atomic_dataset_compact_index
cargo test -p storage --test atomic_dataset_contract
cargo test -p storage --test atomic_dataset_crash
cargo fmt --all -- --check
```

Expected: 全部 PASS；损坏 sidecar 返回 typed error，原始 Session 仍可读取。

- [ ] **Step 5: 提交并创建 #1165 PR**

```bash
git add agent/features/context
git commit -m "feat(context): #1165 持久化 compact sidecar"
git pull --no-rebase origin release/v0.1.0
git push -u origin feat/1165-compact-index-sidecar
```

### Task 4: #1168 per-session 1 / global 5 Scheduler

**Files:**

- Create: `agent/features/context/src/application/compact_scheduler.rs`
- Create: `agent/features/context/src/application/compact_scheduler_tests.rs`
- Modify: `agent/features/context/src/application.rs`
- Modify: `agent/features/context/src/lib.rs`
- Modify: `agent/composition/src/runtime.rs`
- Test: `agent/features/context/src/application/compact_scheduler_tests.rs`

- [ ] **Step 1: 写 barrier 驱动的并发失败测试**

```rust
#[tokio::test]
async fn six_sessions_never_exceed_five_global_or_one_per_session() {
    let harness = SchedulerHarness::new(5);
    let jobs = vec![
        harness.job("s1"),
        harness.job("s1"),
        harness.job("s2"),
        harness.job("s3"),
        harness.job("s4"),
        harness.job("s5"),
        harness.job("s6"),
    ];

    let run = harness.spawn_all(jobs);
    harness.wait_until_five_started().await;

    assert_eq!(harness.max_global_active(), 5);
    assert_eq!(harness.max_active_for("s1"), 1);

    harness.release_all();
    run.await.expect("scheduler completion");
}

#[tokio::test]
async fn must_gap_precedes_background_branch_without_cancelling_in_flight_work() {
    let harness = SchedulerHarness::new(1);
    let first = harness.start_background_leaf("s1").await;
    harness.enqueue_background_branch("s2");
    harness.enqueue_must_gap("s3");
    first.complete();

    assert_eq!(harness.next_started().await.priority, CompactPriority::MustGap);
}
```

- [ ] **Step 2: 运行 scheduler 测试确认失败**

Run:

```bash
cargo test -p context --lib application::compact_scheduler_tests
```

Expected: FAIL，`CompactScheduler` 或 `CompactPriority` 尚不存在。

- [ ] **Step 3: 实现公平队列、双层 permit 与 retry**

```rust
pub const COMPACT_PER_SESSION_LIMIT: usize = 1;
pub const COMPACT_GLOBAL_SESSION_LIMIT: usize = 5;

pub struct CompactScheduler {
    global: Arc<Semaphore>,
    sessions: Mutex<HashMap<SessionId, Arc<Semaphore>>>,
    queues: Mutex<PriorityQueues>,
}

impl CompactScheduler {
    async fn acquire(&self, session_id: &SessionId) -> Result<CompactPermit, SchedulerError> {
        let session = self
            .session_semaphore(session_id)
            .acquire_owned()
            .await?;
        // 先取得 session permit，避免同一 session 的排队 job 占住多个 global permit。
        let global = self.global.clone().acquire_owned().await?;
        Ok(CompactPermit { global, session })
    }
}
```

retry 只接受错误分类：

```rust
match error.kind() {
    Network | RateLimited | Timeout if attempt < 3 => Retry::After(backoff(attempt)),
    ContextTooLong => Retry::ResizeInput,
    Authentication | Permission | InvalidRequest => Retry::StopSession,
    _ => Retry::OpenCircuitAfterThirdFailure,
}
```

- [ ] **Step 4: 验证并发、取消、公平性与 Composition 唯一装配**

Run:

```bash
cargo test -p context --lib application::compact_scheduler
cargo test -p composition --lib
cargo test -p context --tests
cargo fmt --all -- --check
```

Expected: 全部 PASS；测试无短 sleep；`CompactScheduler::new(5)` 只出现在 Composition 生产装配。

- [ ] **Step 5: 提交并创建 #1168 PR**

```bash
git add agent/features/context agent/composition/src/runtime.rs
git commit -m "feat(context): #1168 增加公平 compact scheduler"
git pull --no-rebase origin release/v0.1.0
git push -u origin feat/1168-compact-scheduler
```

### Task 5: #1166 增量 Projection、warm activation 与 Runtime 接入

**Files:**

- Create: `agent/features/context/src/application/summary_projection.rs`
- Create: `agent/features/context/src/application/summary_projection_tests.rs`
- Modify: `agent/features/context/src/application/service.rs`
- Modify: `agent/features/context/src/ports/context_port.rs`
- Modify: `agent/features/context/src/domain.rs`
- Modify: `agent/features/runtime/src/ports/context_port.rs`
- Modify: `agent/features/runtime/src/application/context_coordination.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/compact.rs`
- Test: `agent/features/context/src/application/summary_projection_tests.rs`
- Test: `agent/features/runtime/src/ports/context_port_tests.rs`

- [ ] **Step 1: 写 warm、Must 缺口和第二次 compact 失败测试**

```rust
#[tokio::test]
async fn warm_projection_activation_makes_zero_generator_calls() {
    let fixture = ProjectionFixture::with_ready_projection();

    let outcome = fixture.context.compact(&fixture.manual_request()).await.unwrap();

    assert!(matches!(outcome, CompactOutcome::Committed(_)));
    assert_eq!(fixture.generator.calls(), 0);
}

#[tokio::test]
async fn second_compact_only_generates_uncovered_increment() {
    let mut fixture = ProjectionFixture::with_first_projection(steps(1..=80));
    fixture.append(steps(81..=160)).await;
    fixture.background_build(steps(81..=120)).await;

    fixture.context.compact(&fixture.manual_request()).await.unwrap();

    assert_eq!(fixture.generator.requested_ranges(), vec![range(81..=120)]);
    assert_eq!(fixture.invocations_for(range(1..=80)), 1);
}

#[tokio::test]
async fn cancelled_must_gap_does_not_publish_partial_projection() {
    let fixture = ProjectionFixture::with_must_gap();
    fixture.generator.block_next();
    let call = fixture.spawn_compact();
    fixture.cancel();

    assert!(matches!(call.await.unwrap(), CompactOutcome::Skipped(_)));
    assert_eq!(fixture.index.active_projection(), None);
}
```

- [ ] **Step 2: 运行 Context / Runtime 测试确认失败**

Run:

```bash
cargo test -p context --lib application::summary_projection_tests
cargo test -p runtime --lib ports::context_port_tests
```

Expected: FAIL，`compact_status`、`SummaryProjectionService` 或 warm activation 尚不存在。

- [ ] **Step 3: 实现 append 后调度、build_window projection 和 compact 发布**

```rust
#[async_trait]
impl ContextPort for ContextApplicationService {
    async fn append_and_persist(
        &self,
        append: &ContextAppend,
    ) -> Result<AppendReceipt, ContextAppendError> {
        let receipt = self.session.append_finalized(append).await?;
        self.projection.schedule_newly_unprotected(&receipt).await?;
        Ok(receipt)
    }

    async fn compact(&self, request: &CompactRequest) -> Result<CompactOutcome, ContextPortError> {
        self.projection.activate_or_fill_gap(request).await
    }

    async fn compact_status(
        &self,
        session_id: &SessionId,
    ) -> Result<CompactStatus, ContextPortError> {
        self.projection.status(session_id).await
    }
}
```

`build_window` 必须按 `summarized prefix → uncovered raw → recent raw` 完整组装；缺口可容纳时不等待，Must 且不可容纳时只等待当前 session 一个 job。

- [ ] **Step 4: 验证相邻 Context / Runtime 边界和 L4 场景**

Run:

```bash
cargo test -p context --lib application::summary_projection
cargo test -p context --tests
cargo test -p runtime --lib ports::context_port_tests
cargo test -p runtime --lib application::chat::looping::compact
cargo fmt --all -- --check
```

Expected: 全部 PASS；Runtime 不导入 `SummaryShard`、`CompactManifest` 或 scheduler queue 类型。

- [ ] **Step 5: 提交并创建 #1166 PR**

```bash
git add agent/features/context agent/features/runtime
git commit -m "feat(context): #1166 接入增量 compact projection"
git pull --no-rebase origin release/v0.1.0
git push -u origin feat/1166-compact-projection
```

### Task 6: #1167 Compact Attempt Usage、SDK 与 TUI

**Files:**

- Modify: `agent/features/context/src/domain/compact/summary_tree.rs`
- Modify: `agent/features/context/src/ports/context_port.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/events.rs`
- Modify: `agent/features/runtime/src/adapters/event_projection.rs`
- Modify: `packages/sdk/src/chat_event.rs`
- Modify: `apps/cli/src/tui/app/event.rs`
- Modify: `apps/cli/src/tui/effect/session/processing/event_mapping.rs`
- Modify: `apps/cli/src/tui/adapter/agent_event.rs`
- Modify: `apps/cli/src/tui/model/conversation/compact_progress.rs`
- Modify: `apps/cli/src/tui/model/conversation/usage.rs`
- Test: `agent/features/runtime/src/adapters/event_projection_tests.rs`
- Test: `apps/cli/src/tui/adapter/agent_event/tests.rs`

- [ ] **Step 1: 写 usage 守恒与每层字段保留失败测试**

```rust
#[test]
fn compact_usage_summary_equals_sum_of_attempts() {
    let records = vec![
        attempt_for_phase(CompactPhase::Leaf, 100, 20),
        attempt_for_phase(CompactPhase::BranchReduce, 40, 10),
    ];

    let summary = CompactUsageSummary::from_attempts(&records);

    assert_eq!(summary.input_tokens, 140);
    assert_eq!(summary.output_tokens, 30);
    assert_eq!(summary.calls, 2);
}

#[test]
fn compact_retry_reuses_model_invocation_id_and_counts_each_attempt_once() {
    let id = ModelInvocationId::new_for_test();
    let records = vec![
        attempt(id, 1, UsageSource::ProviderReported, 100, 0),
        attempt(id, 2, UsageSource::ProviderReported, 100, 20),
        attempt(id, 2, UsageSource::ProviderReported, 100, 20),
    ];

    let summary = CompactUsageSummary::from_attempts(&records);

    assert_eq!(summary.input_tokens, 200);
    assert_eq!(summary.output_tokens, 20);
    assert_eq!(summary.logical_invocations, 1);
    assert_eq!(summary.attempts, 2);
}

#[test]
fn runtime_projection_preserves_compact_usage_fields() {
    let event = runtime_compact_status_event();
    let sdk = project_event(event);

    let ChatEvent::CompactStatus { status } = sdk else {
        panic!("expected compact status");
    };
    assert_eq!(status.reused_shards, 4);
    assert_eq!(status.pending_tokens, 12_000);
    assert_eq!(status.usage.leaf.input_tokens, 100);
    assert_eq!(status.usage.branch_reduce.output_tokens, 10);
}

#[test]
fn tui_adapter_keeps_unknown_usage_distinct_from_zero() {
    let intent = map_agent_event(&UiEvent::CompactStatus {
        status: compact_status_with_unknown_usage(),
    });

    assert_eq!(intent.compact_usage().source, UsageSourceView::Unknown);
    assert_eq!(intent.compact_usage().normalized_total_tokens, None);
}
```

- [ ] **Step 2: 运行 Context、Runtime、SDK、TUI 定向测试确认失败**

Run:

```bash
cargo test -p context compact_usage
cargo test -p runtime event_projection
cargo test -p sdk chat_event
cargo test -p cli agent_event
```

Expected: FAIL，新的 status / usage 字段尚未存在。

- [ ] **Step 3: 增加只读 CompactStatus 与 usage 投影**

```rust
pub struct CompactStatusView {
    pub summarized_steps: u64,
    pub pending_tokens: u64,
    pub reused_shards: u64,
    pub phase: Option<CompactPhaseView>,
    pub circuit_breaker_open: bool,
    pub usage: CompactUsageSummaryView,
}

pub enum ChatEvent {
    CompactStatus {
        status: CompactStatusView,
    },
    // existing events remain unchanged
}
```

TUI `UsageSummary` 增加 `compact: CompactUsageSummaryView`，`/cost` 与
`/compact` 从 model 读取，不直接读 sidecar。Context sidecar 只保存 token /
attempt facts，不保存 Price / Cost；`/cost` 的 compact 金额在读取时复用
Runtime pricing 派生并标记 estimated。Context **NEVER** 持有 Audit
`UsagePort` / Runtime `UsageSink`；若装配成功 compact invocation 的 Audit
投影，必须由 Composition 在 `SummaryGenerator` 外层聚合后 best-effort
提交，Audit 结果不影响 compact checkpoint。后台成功不产生 system message；
circuit breaker 从 closed 变 open 时只通知一次。

- [ ] **Step 4: 跑每层相邻契约与场景测试**

Run:

```bash
cargo test -p context compact_usage
cargo test -p runtime event_projection
cargo test -p sdk
cargo test -p cli --lib compact
cargo test -p cli --lib usage
cargo fmt --all -- --check
```

Expected: 全部 PASS；unknown / estimated / provider-reported 三态不被压成零值；
相同 `(ModelInvocationId, attempt)` 不重复计数，Context crate 不依赖 Audit
或 Runtime UsageSink。

- [ ] **Step 5: 提交并创建 #1167 PR**

```bash
git add agent/features/context agent/features/runtime packages/sdk apps/cli
git commit -m "feat(context): #1167 贯通 compact usage 与状态展示"
git pull --no-rebase origin release/v0.1.0
git push -u origin feat/1167-compact-usage
```

### Task 7: #1119 Legacy map-reduce 迁移、Guard / Verify 与退役

**Files:**

- Modify: `agent/features/context/src/adapters/compact_summary.rs`
- Modify: `agent/features/context/src/adapters/compact_summary_tests.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/compact.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/events.rs`
- Modify: `packages/sdk/src/chat_event.rs`
- Modify: `apps/cli/src/tui/model/conversation/compact_progress.rs`
- Create: `agent/features/context/tests/persistent_compact_lifecycle.rs`
- Modify: `.agents/hooks/check-cargo-dependency-graph.sh`
- Modify: `.agents/hooks/check-crate-api-boundary.sh`
- Modify: `.agents/hooks/check-production-reachability.sh`
- Modify: `docs/design/03-engineering/01-architecture-guards.md`
- Test: `agent/features/context/tests/persistent_compact_lifecycle.rs`

- [ ] **Step 1: 写完整生命周期与 legacy 不可达失败测试**

```rust
#[tokio::test]
async fn restart_then_second_compact_reuses_completed_prefix_and_usage() {
    let fixture = PersistentLifecycleFixture::new();
    fixture.append(steps(1..=100)).await;
    fixture.first_compact().await;
    let first_usage = fixture.compact_usage();

    fixture.append(steps(101..=160)).await;
    fixture.background_build_through(step(120)).await;
    let resumed = fixture.restart().await;
    resumed.second_compact().await;

    assert_eq!(resumed.invocations_for(range(1..=80)), 1);
    assert_eq!(resumed.usage_for(range(1..=80)), first_usage);
    assert!(resumed.raw_session_contains(steps(1..=160)).await);
}

#[test]
fn legacy_synchronous_map_reduce_is_not_production_reachable() {
    assert!(!production_symbols().contains("compact_messages_map_reduce"));
}
```

- [ ] **Step 2: 运行场景和 guard，保存首次失败证据**

Run:

```bash
cargo test -p context --test persistent_compact_lifecycle
.agents/hooks/check-production-reachability.sh
.agents/hooks/check-architecture-guards.sh
```

Expected: lifecycle FAIL 或 guard FAIL，因为同步 map-reduce 仍可达。

- [ ] **Step 3: 迁移 backfill 并删除 legacy 同步主路径**

生产入口统一为：

```rust
match context.compact(request).await? {
    CompactOutcome::Committed(result) => apply_once(result),
    CompactOutcome::Skipped(reason) => continue_without_retry(reason),
    CompactOutcome::Failed(error) => report_typed_failure(error),
}
```

删除或私有化：

```rust
compact_messages_map_reduce
build_summary_text
Vec<String> sub_summaries
```

旧历史通过 `CompactJobKind::BackfillLeaf` 进入 scheduler；completed/total 由 manifest coverage 产生，不再在请求开始前伪报 progress。

- [ ] **Step 4: 故意违规验证 guard**

临时在 Context domain 引入 Runtime dependency，运行：

```bash
.agents/hooks/check-cargo-dependency-graph.sh
```

Expected: exit code 2，明确拒绝 `context → runtime`。立即恢复故意违规，再运行完整 guard，Expected: PASS。

- [ ] **Step 5: 运行完整 L0–L4 门禁**

Run:

```bash
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
.agents/hooks/check-production-reachability.sh
.agents/hooks/check-architecture-guards.sh
```

Expected: 全部 PASS。PR Test plan 必须列出首次失败、每层相邻契约和 changed-lines coverage 信号。

- [ ] **Step 6: 执行 L5 真实 Provider smoke**

对保存的 7 / 9 / 22 chunk 历史分别记录：

```text
cold backfill: calls / input / output / normalized total / elapsed
warm activation: calls=0 / elapsed
second compact: stable-prefix calls=0 / new-range calls / tokens / elapsed
```

Expected:

- warm activation 零 Provider 调用，真实性能 p95 ≤ 500ms；
- 第二次 compact 对稳定前缀零调用、零重复 usage；
- Must 单缺口最多一个 Provider round trip。

- [ ] **Step 7: 提交并创建 #1119 PR**

```bash
git add agent/features/context agent/features/runtime packages/sdk apps/cli .agents/hooks docs/design/03-engineering/01-architecture-guards.md
git commit -m "refactor(context): #1119 退役同步 compact map-reduce"
git pull --no-rebase origin release/v0.1.0
git push -u origin refactor/1119-compact-backfill-retirement
```

## 8. 每个 PR 的统一收尾检查

- [ ] Issue 开发前“代码—Target 差异”矩阵已基于最新 release 回填。
- [ ] 测试先失败、生产实现后通过；首次 CI 失败证据未被重跑覆盖。
- [ ] 对应 owning layer、相邻契约和场景层级完整。
- [ ] 没有新的重复 usage 算法、重复 filesystem atomic protocol 或 Runtime / Context 双状态源。
- [ ] `git diff --check`、fmt、定向测试、workspace 门禁、Clippy、production reachability 和 architecture guards 通过。
- [ ] `git pull --no-rebase origin release/v0.1.0` 后重新验证。
- [ ] PR 使用 `.github/pull_request_template.md`，Refs 指向唯一叶子 Issue，base 为 `release/v0.1.0`。
- [ ] PR 创建后等待用户 review；没有当前会话对具体 PR head 的授权时不合并、不关闭 Issue。

## 9. Spec 自检

- Spec coverage：#1162 的保真、延迟、持久化、第一次 / 第二次 compact、per-session 1 / global 5、usage、失败 / 取消 / 重试、版本 / GC、端口所有权和 L0–L5 验收均映射到 Task 1–7。
- Placeholder scan：计划没有未决实现占位；每个写代码步骤都给出目标签名、测试或迁移代码。
- Type consistency：`SummaryShard`、`CompactProjection`、`SummaryGenerator`、`CompactIndex`、`CompactScheduler`、`CompactStatus`、`CompactAttemptUsage` 与 Target spec 使用同一名称；invocation identity 复用 SDK `ModelInvocationId`。
