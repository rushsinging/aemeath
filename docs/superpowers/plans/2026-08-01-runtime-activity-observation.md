# Runtime Activity 统一观测 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 建立 Runtime-owned ActivityCoordinator、typed Activity observation/snapshot 与 TUI 低噪声摘要，并物理退役旧 RunStatus timing/spinner 活动展示双轨。

**Architecture:** `Run`、`RunStep`、`ToolCall` 与各能力 typed outcome 继续拥有业务事实；per-Run `ActivityCoordinator` 只维护非持久化观察注册表，消费领域事件和 Runtime application facts，向 SDK 发布完整 ActivityView 增量与快照。TUI 经两层 ACL 保存 Activity 事实镜像，由纯 `ActivitySummaryAssembler` 选择 Main Run 根活动、当前 phase 和最多一个稳定 detail；内容、terminal、Interaction 和 ToolCall 输出协议保持独立。

**Tech Stack:** Rust、Tokio、UUIDv7、serde/schemars、Runtime 六边形架构、SDK Published Language、TUI TEA/ratatui、cargo nextest/cargo test、架构 Guard shell/xtask。

**Design Spec:** `docs/superpowers/specs/2026-08-01-runtime-activity-observation-design.md`

---

## 文件结构

### Runtime

- Create: `agent/features/runtime/src/application/activity.rs` — Activity application façade 与受控 re-export。
- Create: `agent/features/runtime/src/application/activity/model.rs` — `ActivityObservation`、kind/state/source/detail/audience/timing 与 typed commands。
- Create: `agent/features/runtime/src/application/activity/coordinator.rs` — per-Run registry、revision、计时、父子/source 校验、snapshot 与 terminal safety close。
- Create: `agent/features/runtime/src/application/activity/coordinator_tests.rs` — Coordinator L1 测试；固定 ID/Clock，不使用 sleep。
- Create: `agent/features/runtime/src/application/activity/run_events.rs` — `RunDomainEvent` 到 Run root/phase Activity command 的无状态翻译。
- Create: `agent/features/runtime/src/application/activity/run_events_tests.rs` — Run transition/terminal 的相邻契约测试。
- Modify: `agent/features/runtime/src/application.rs` — 注册 `activity` 模块。
- Modify: `agent/features/runtime/src/application/run/context.rs` — `RuntimeContext` 持有 per-Run `ActivityCoordinator`。
- Modify: `agent/features/runtime/src/application/run/context_factory.rs` — 唯一构造 Coordinator。
- Modify: `agent/features/runtime/src/application/loop_engine/engine.rs` — Run mutation 后原子 observe/publish。
- Modify: `agent/features/runtime/src/application/model/**` — Model invocation start/retry/wait/finish Activity。
- Modify: `agent/features/runtime/src/application/tool/**` 与 `application/loop_engine/chat/tools.rs` — Tool/Child Run Activity。
- Modify: `agent/features/runtime/src/application/hook/**` 与 `application/loop_engine/**` Hook 调用点 — Hook Activity。
- Modify: `agent/features/runtime/src/application/context/**` 与 compact 调用点 — Context/Compact Activity。
- Modify: `agent/features/runtime/src/application/interaction/**` — Interaction wait/resume/terminal Activity。
- Modify: `agent/features/runtime/src/adapters/sdk_event_mapper.rs` — Activity 到 SDK PL 的唯一映射。
- Modify: `agent/features/runtime/src/adapters/sdk_event_mapper_tests.rs` — Activity adapter 契约。
- Modify: `agent/features/runtime/src/adapters/sdk_event_sink.rs` — 发布 Activity change/snapshot。

### SDK

- Create: `packages/sdk/src/activity.rs` — `ActivityId`、Activity enums/view/snapshot/change。
- Create: `packages/sdk/src/activity_tests.rs` — serde/schema/shape/revision 契约。
- Modify: `packages/sdk/src/chat_event.rs` — `ActivityChanged` / `ActivitySnapshot`，最终收窄 `RunTransitioned`。
- Modify: `packages/sdk/src/chat.rs`、`packages/sdk/src/lib.rs`、`packages/sdk/src/wire.rs` — re-export 与 schema 注册。

### TUI

- Modify: `apps/cli/src/tui/adapter/tui_runtime_event.rs` — TUI-owned Activity DTO。
- Modify: `apps/cli/src/tui/adapter/event_mapping.rs` 与 `event_mapping_tests.rs` — SDK Activity 的穷举结构转换。
- Modify: `apps/cli/src/tui/adapter/agent_event.rs` 与相邻测试 — Activity event → Intent。
- Create: `apps/cli/src/tui/model/conversation/activity.rs` — Activity 事实镜像、revision/gap/snapshot replace。
- Create: `apps/cli/src/tui/model/conversation/activity_tests.rs` — reducer/model 契约。
- Modify: `apps/cli/src/tui/model/conversation/model.rs`、`intent.rs`、`change.rs` — 唯一 Model 写入口。
- Create: `apps/cli/src/tui/view_assembler/activity_summary.rs` — 低噪声摘要选择、并行聚合与稳定门槛。
- Create: `apps/cli/src/tui/view_assembler/activity_summary_tests.rs` — 纯摘要策略测试。
- Modify: `apps/cli/src/tui/view_assembler/live_status.rs` — 只消费 Activity summary。
- Modify: `apps/cli/src/tui/view_state/run_activity.rs` — 只保留动画和 monotonic interpolation anchor。
- Modify: `apps/cli/src/tui/app/scenario_tests/chat.rs` — 全链路稳定摘要场景。
- Modify: `apps/cli/src/tui/architecture_tests.rs` — 禁止旧活动字段和旁路推断。

### 文档与 Guard

- Modify: `docs/design/01-system/02-ubiquitous-language.md`
- Modify: `docs/design/01-system/03-context-map.md`
- Modify: `docs/design/02-modules/runtime/{01-domain-model,03-loop-and-state-machine,06-ports-and-adapters,07-runtime-ownership-and-assembly}.md`
- Modify: `docs/design/02-modules/tools/02-ports-and-lifecycle.md`
- Modify: `docs/design/02-modules/hook/01-run-loop-integration.md`
- Modify: `docs/design/02-modules/tui/{01-architecture-and-dataflow,02-model,03-event-flow-and-acl,04-view-layer}.md`
- Modify: `docs/design/03-engineering/03-migration-governance.md`
- Modify: `.agents/hooks/check-architecture-guards.sh`、`.agents/architecture-guard-registry.json`、`docs/design/03-engineering/01-architecture-guards.md`（若沿用现有 TUI guard 则只扩规则，不新增重复脚本）。

---

### Task 1: 发布 SDK Activity Published Language

**Files:**
- Create: `packages/sdk/src/activity.rs`
- Create: `packages/sdk/src/activity_tests.rs`
- Modify: `packages/sdk/src/chat_event.rs`
- Modify: `packages/sdk/src/chat.rs`
- Modify: `packages/sdk/src/lib.rs`
- Modify: `packages/sdk/src/wire.rs`

- [ ] **Step 1: 编写失败的 Activity PL shape 测试**

在 `activity_tests.rs` 固定覆盖所有 kind/state/source/detail/audience、完整 `ActivityView`、`ActivityChanged` 和 `ActivitySnapshot`。关键断言如下：

```rust
#[test]
fn activity_view_round_trip_preserves_identity_revision_and_timing() {
    let view = activity_fixture();
    let encoded = serde_json::to_value(&view).expect("encode activity");
    let decoded: ActivityView = serde_json::from_value(encoded).expect("decode activity");
    assert_eq!(decoded, view);
    assert_eq!(decoded.revision, 7);
    assert_eq!(decoded.timing.total_elapsed_ms, 12_000);
    assert_eq!(decoded.timing.active_elapsed_ms, 9_000);
}

#[test]
fn activity_snapshot_carries_complete_views_at_one_run_revision() {
    let snapshot = ActivitySnapshotView {
        run_id: RunId::from_string("01900000-0000-7000-8000-000000000001"),
        revision: 9,
        activities: vec![activity_fixture()],
    };
    assert_eq!(snapshot.activities[0].run_id, snapshot.run_id);
    assert_eq!(snapshot.revision, 9);
}
```

- [ ] **Step 2: 运行 SDK 测试并确认因类型缺失失败**

Run: `cargo test -p sdk activity -- --nocapture`  
Expected: FAIL，错误指向 `ActivityView` / `ActivitySnapshotView` 未定义。

- [ ] **Step 3: 实现 SDK Activity PL**

在 `activity.rs` 定义：

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ActivityView {
    pub id: ActivityId,
    pub run_id: RunId,
    pub run_step_id: Option<RunStepId>,
    pub parent_activity_id: Option<ActivityId>,
    pub source: ActivitySourceView,
    pub kind: ActivityKindView,
    pub state: ActivityStateView,
    pub detail: ActivityDetailView,
    pub audience: ActivityAudienceView,
    pub revision: u64,
    pub timing: ActivityTimingView,
}
```

并定义 `ActivityChangeKind::{Started,Updated,Finished}`、`ActivitySnapshotView`、全部封闭枚举及 `ActivityId` UUIDv7 newtype。`ChatEvent` 增加：

```rust
ActivityChanged {
    kind: ActivityChangeKind,
    activity: ActivityView,
},
ActivitySnapshot(ActivitySnapshotView),
```

更新 façade 和 wire schema 注册；不得在 SDK 放 TUI 文案、颜色或 visible 字段。

- [ ] **Step 4: 运行 SDK Activity 与 wire 测试**

Run: `cargo test -p sdk activity -- --nocapture && cargo test -p sdk wire -- --nocapture`  
Expected: PASS。

- [ ] **Step 5: 提交 SDK PL**

```bash
git add packages/sdk/src
git commit -m "feat(sdk): #742 发布 Activity 观测契约"
```

---

### Task 2: 建立 ActivityCoordinator 领域无关观测内核

**Files:**
- Create: `agent/features/runtime/src/application/activity.rs`
- Create: `agent/features/runtime/src/application/activity/model.rs`
- Create: `agent/features/runtime/src/application/activity/coordinator.rs`
- Create: `agent/features/runtime/src/application/activity/coordinator_tests.rs`
- Modify: `agent/features/runtime/src/application.rs`
- Modify: `agent/features/runtime/src/lib.rs`

- [ ] **Step 1: 编写 Coordinator 失败测试**

覆盖 start/update/wait/resume/finish、终态不可转出、unknown update、父活动/run/step/source 校验、revision 单调、同 source live 唯一、snapshot 和 close_run。使用 `FixedActivityClock` 与 `FixedActivityIdSource`：

```rust
#[test]
fn finish_freezes_timing_and_terminal_state() {
    let harness = ActivityHarness::new();
    let activity_id = harness.start_tool("Read");
    harness.clock.advance_ms(2_000);
    harness.finish(activity_id.clone(), ActivityTerminal::Succeeded).unwrap();
    harness.clock.advance_ms(3_000);

    let activity = harness.coordinator.snapshot().find(&activity_id).unwrap();
    assert_eq!(activity.state, ActivityState::Succeeded);
    assert_eq!(activity.timing.total_elapsed_ms, 2_000);
    assert_eq!(activity.revision, 2);
}

#[test]
fn close_run_never_converts_live_activity_to_success() {
    let harness = ActivityHarness::new();
    let activity_id = harness.start_tool("Bash");
    harness.coordinator.close_run(ActivityTerminal::Terminated).unwrap();
    assert_eq!(
        harness.coordinator.snapshot().find(&activity_id).unwrap().state,
        ActivityState::Terminated
    );
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run: `cargo test -p runtime activity::coordinator -- --nocapture`  
Expected: FAIL，Activity application 模块尚不存在。

- [ ] **Step 3: 实现 model 与 Coordinator**

实现 `ActivityObservation`、typed commands、`ActivityRegistry` 和：

```rust
pub(crate) struct ActivityCoordinator {
    run_id: RunId,
    clock: Arc<dyn ActivityClock>,
    ids: Arc<dyn ActivityIdSource>,
    registry: Mutex<ActivityRegistry>,
    sink: ChatEventSinkHandle,
}
```

每次 mutation 在同一锁内生成完整 observation 和单调 revision，释放锁后发布 `RuntimeStreamEvent::ActivityChanged`；`snapshot()` 按 parent/start 顺序稳定排序。错误使用明确的 `ActivityError`，错误消息中文。

- [ ] **Step 4: 运行 Coordinator 测试**

Run: `cargo test -p runtime activity::coordinator -- --nocapture`  
Expected: PASS。

- [ ] **Step 5: 提交 Activity 内核**

```bash
git add agent/features/runtime/src/application/activity* agent/features/runtime/src/application.rs agent/features/runtime/src/lib.rs
git commit -m "feat(runtime): #742 建立统一 Activity 观测内核"
```

---

### Task 3: 由统一 RuntimeContextFactory 装配 per-Run Coordinator

**Files:**
- Modify: `agent/features/runtime/src/application/run/context.rs`
- Modify: `agent/features/runtime/src/application/run/context_factory.rs`
- Modify: `agent/features/runtime/src/application/run/context_factory_tests.rs`
- Modify: `agent/composition/src/runtime_tests.rs`

- [ ] **Step 1: 编写失败的装配与隔离测试**

```rust
#[tokio::test]
async fn factory_creates_one_activity_coordinator_per_run() {
    let first = factory().create(root_request()).await.unwrap();
    let second = factory().create(root_request()).await.unwrap();
    assert_ne!(first.activities().run_id(), second.activities().run_id());
    assert!(!Arc::ptr_eq(first.activities(), second.activities()));
}

#[tokio::test]
async fn child_activity_coordinator_uses_child_run_identity() {
    let child = factory().create(child_request()).await.unwrap();
    assert_eq!(child.activities().run_id(), child.run_id());
}
```

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p runtime context_factory activity -- --nocapture && cargo test -p composition runtime activity -- --nocapture`  
Expected: FAIL，`RuntimeContext::activities()` 不存在。

- [ ] **Step 3: 只在 factory 创建 Coordinator**

给 `RuntimeContext` 增加私有 `Arc<ActivityCoordinator>` 字段和 crate-private accessor；`RuntimeContextFactory` 使用已绑定 event sink、clock、run id 构造。禁止把 Coordinator 塞进 `RuntimeContextParts` 或让调用方手填。

- [ ] **Step 4: 运行 Runtime/Composition 装配测试**

Run: `cargo test -p runtime context_factory -- --nocapture && cargo test -p composition runtime -- --nocapture`  
Expected: PASS。

- [ ] **Step 5: 提交装配**

```bash
git add agent/features/runtime/src/application/run agent/composition/src/runtime_tests.rs
git commit -m "refactor(runtime): #742 按 Run 装配 ActivityCoordinator"
```

---

### Task 4: 将 Run root 与 phase 原子映射为 Activity

**Files:**
- Create: `agent/features/runtime/src/application/activity/run_events.rs`
- Create: `agent/features/runtime/src/application/activity/run_events_tests.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/engine.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/engine_tests.rs`

- [ ] **Step 1: 编写失败的 Run transition 场景测试**

测试 `Created → DrainingInput → PreparingContext → InvokingModel` 产生：一个根 Activity、连续 phase Activity、旧 phase 先终结、新 phase 后开始；terminal 后没有 live Activity。必须同时断言没有重复根活动和成功/终止冲突。

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p runtime run_activity -- --nocapture`  
Expected: FAIL，RunDomainEvent 尚未进入 ActivityCoordinator。

- [ ] **Step 3: 实现 `observe_run_events` 与 Engine 原子顺序**

`run_events.rs` 穷举 `RunDomainEvent`，将 Started/Transitioned/terminal 映射为 typed command。Engine 的统一 mutation helper 固定顺序：

```rust
let events = run.drain_events();
context.activities().observe_run_events(&events)?;
event_sink.emit_domain(events).await?;
```

不得在 adapter 或 TUI 补造 phase。Run terminal 后调用 `close_run`。

- [ ] **Step 4: 运行 Run domain/application 测试**

Run: `cargo test -p runtime agent_run -- --nocapture && cargo test -p runtime run_activity -- --nocapture`  
Expected: PASS。

- [ ] **Step 5: 提交 Run Activity 链**

```bash
git add agent/features/runtime/src/application/activity agent/features/runtime/src/application/loop_engine
git commit -m "feat(runtime): #742 原子观察 Run Activity"
```

---

### Task 5: 接通 Model、Tool 与 Child Run leaf Activity

**Files:**
- Modify: `agent/features/runtime/src/application/model/**`
- Modify: `agent/features/runtime/src/application/tool/**`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/tools.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/agent_calls.rs`
- Test: 对应同级 `*_tests.rs`

- [ ] **Step 1: 先写 Model 与 Tool 失败测试**

覆盖：Model start→retry update→streaming→success；Tool approval wait→running→terminal；并行三个工具各自 identity；Agent Tool 建 ChildRun leaf 并关联 child run id。断言 Activity 不修改 ToolCall/Run 结果。

- [ ] **Step 2: 运行定向测试确认失败**

Run: `cargo test -p runtime model_activity -- --nocapture && cargo test -p runtime tool_activity -- --nocapture`  
Expected: FAIL，调用点尚未创建 Activity。

- [ ] **Step 3: 在 application coordinator 调用点接线**

Model coordinator 在 invoke 前 start，在 retry 更新 attempt/waiting，在首有效 stream resume，在 typed completion finish。Tool round coordinator 在 approval/execution/suspension/terminal 更新 Tool Activity；Agent Tool 同时建立 `ChildRun` detail，使用 `ActivitySource::ChildRun(child_run_id)`。外部 Provider/Tool 不取得 Activity callback。

- [ ] **Step 4: 运行 Model/Tool/Agent 测试**

Run: `cargo test -p runtime model_activity -- --nocapture && cargo test -p runtime tool_activity -- --nocapture && cargo test -p runtime agent_calls -- --nocapture`  
Expected: PASS。

- [ ] **Step 5: 提交主要 leaf Activity**

```bash
git add agent/features/runtime/src/application/model agent/features/runtime/src/application/tool agent/features/runtime/src/application/loop_engine/chat
git commit -m "feat(runtime): #742 统一模型与工具 Activity"
```

---

### Task 6: 接通 Hook、Compact、Interaction 与 finalization/control Activity

**Files:**
- Modify: `agent/features/runtime/src/application/hook/**`
- Modify: `agent/features/runtime/src/application/context/**`
- Modify: `agent/features/runtime/src/application/interaction/**`
- Modify: `agent/features/runtime/src/application/loop_engine/**`
- Test: 对应同级 `*_tests.rs`

- [ ] **Step 1: 编写每个 owner 的失败测试**

覆盖 Hook attempt/blocked/failure、Compact stage/progress、Interaction waiting/resume/cancel、FinalizingStep、CancellingStep、Terminating；验证等待时 `active_elapsed_ms` 暂停、`total_elapsed_ms` 继续。

- [ ] **Step 2: 运行定向测试确认失败**

Run: `cargo test -p runtime hook_activity -- --nocapture && cargo test -p runtime compact_activity -- --nocapture && cargo test -p runtime interaction_activity -- --nocapture`  
Expected: FAIL。

- [ ] **Step 3: 在各 Runtime application owner 接线**

Hook/Context 只返回 typed outcome/progress；Runtime owner 调 Coordinator。Interaction request 创建 Waiting activity，匹配 reply 后 resume，取消/Run control 按 typed cause 终结。Finalizer/control 使用 Run phase Activity，不创建内部子动作噪声。

- [ ] **Step 4: 运行 owner 与 Loop 测试**

Run: `cargo test -p runtime hook_activity -- --nocapture && cargo test -p runtime compact_activity -- --nocapture && cargo test -p runtime interaction_activity -- --nocapture && cargo test -p runtime loop_engine -- --nocapture`  
Expected: PASS。

- [ ] **Step 5: 提交剩余 Activity 覆盖**

```bash
git add agent/features/runtime/src/application
git commit -m "feat(runtime): #742 覆盖完整运行 Activity"
```

---

### Task 7: 映射并发布 SDK Activity change/snapshot

**Files:**
- Modify: `agent/features/runtime/src/application/loop_engine/chat/events.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/events_tests.rs`
- Modify: `agent/features/runtime/src/adapters/sdk_event_mapper.rs`
- Modify: `agent/features/runtime/src/adapters/sdk_event_mapper_tests.rs`
- Modify: `agent/features/runtime/src/adapters/sdk_event_sink.rs`

- [ ] **Step 1: 编写失败的 Runtime→SDK 相邻契约测试**

构造每个 Runtime Activity kind/state/detail，断言 `ActivityView` 字段无损；snapshot 保持 run/revision/稳定排序；安全字段中不存在完整 tool args、Hook stdout/stderr 或 provider raw response。

- [ ] **Step 2: 运行 adapter 测试确认失败**

Run: `cargo test -p runtime sdk_event_mapper activity -- --nocapture`  
Expected: FAIL，Runtime stream/sink 尚未支持 Activity。

- [ ] **Step 3: 实现唯一 SDK mapper 与 sink 发布**

`RuntimeStreamEvent` 增加内部 `ActivityChanged` / `ActivitySnapshot`；`sdk_event_mapper` 穷举转换，禁止 JSON round-trip 或 Debug 字符串。stream 建立/Run activation 后发布初始 snapshot；revision gap 修复只依赖 snapshot。

- [ ] **Step 4: 运行 Runtime adapter 和 SDK contract**

Run: `cargo test -p runtime sdk_event_mapper -- --nocapture && cargo test -p sdk activity -- --nocapture`  
Expected: PASS。

- [ ] **Step 5: 提交 SDK 发布链**

```bash
git add agent/features/runtime/src/application/loop_engine/chat agent/features/runtime/src/adapters
git commit -m "feat(runtime): #742 发布 Activity 增量与快照"
```

---

### Task 8: 建立 TUI-owned Activity DTO 与两层 ACL

**Files:**
- Modify: `apps/cli/src/tui/adapter/tui_runtime_event.rs`
- Modify: `apps/cli/src/tui/adapter/event_mapping.rs`
- Modify: `apps/cli/src/tui/adapter/event_mapping_tests.rs`
- Modify: `apps/cli/src/tui/adapter/agent_event.rs`
- Modify: `apps/cli/src/tui/adapter/agent_event_tests.rs`
- Modify: `apps/cli/src/tui/effect/session/processing/{event_mapping.rs,logging.rs}`

- [ ] **Step 1: 编写 SDK→TUI DTO 与 DTO→Intent 失败测试**

对每个 Activity enum variant 穷举断言；`UiEvent` 之后不得包含 `sdk::*`；snapshot 和 change 产生显式 Conversation Intent，不能 Nop/wildcard。

- [ ] **Step 2: 运行 TUI adapter 测试确认失败**

Run: `cargo test -p cli tui::adapter::event_mapping activity -- --nocapture && cargo test -p cli tui::adapter::agent_event activity -- --nocapture`  
Expected: FAIL。

- [ ] **Step 3: 实现 TUI-owned DTO 与两层转换**

第一层逐字段构造 `TuiActivityObservation`；第二层产生：

```rust
ConversationIntent::ObserveActivity(ActivityChangedIntent { kind, activity })
ConversationIntent::ReplaceActivitySnapshot(ActivitySnapshotIntent { snapshot })
```

logging 只记录 identity/kind/state/revision/duration，不记录 detail 中潜在正文。

- [ ] **Step 4: 运行 adapter/ACL 测试**

Run: `cargo test -p cli tui::adapter -- --nocapture`  
Expected: PASS。

- [ ] **Step 5: 提交 TUI ACL**

```bash
git add apps/cli/src/tui/adapter apps/cli/src/tui/effect/session/processing
git commit -m "feat(tui): #742 接入 typed Activity ACL"
```

---

### Task 9: 在 ConversationModel 建立 Activity 事实镜像

**Files:**
- Create: `apps/cli/src/tui/model/conversation/activity.rs`
- Create: `apps/cli/src/tui/model/conversation/activity_tests.rs`
- Modify: `apps/cli/src/tui/model/conversation/model.rs`
- Modify: `apps/cli/src/tui/model/conversation/intent.rs`
- Modify: `apps/cli/src/tui/model/conversation/change.rs`
- Modify: `apps/cli/src/tui/update/root_reducer.rs`

- [ ] **Step 1: 编写 Model 失败测试**

覆盖 full-view upsert、低 revision 忽略、revision gap 标 stale、snapshot 原子 replace、terminal 不回滚、Main/Sub 隔离、reset 清空。断言 Tool/Hook/RunStatus 内容事件不直接写 Activity Model。

- [ ] **Step 2: 运行 Model 测试确认失败**

Run: `cargo test -p cli tui::model::conversation::activity -- --nocapture`  
Expected: FAIL。

- [ ] **Step 3: 实现 ActivityObservationModel**

保持核心字段私有，只向 ViewAssembler 发布不可变 slice/read view。所有 mutation 只由 root reducer 调用；change 仅标记 status/output dirty，不产生副作用。gap 时保持最后完整事实并设置 stale，等待 snapshot，不猜测状态。

- [ ] **Step 4: 运行 Model/reducer 测试**

Run: `cargo test -p cli tui::model::conversation -- --nocapture && cargo test -p cli tui::update::root_reducer -- --nocapture`  
Expected: PASS。

- [ ] **Step 5: 提交 TUI Model**

```bash
git add apps/cli/src/tui/model/conversation apps/cli/src/tui/update/root_reducer.rs
git commit -m "feat(tui): #742 镜像 Runtime Activity 事实"
```

---

### Task 10: 实现低噪声 ActivitySummaryAssembler

**Files:**
- Create: `apps/cli/src/tui/view_assembler/activity_summary.rs`
- Create: `apps/cli/src/tui/view_assembler/activity_summary_tests.rs`
- Modify: `apps/cli/src/tui/view_assembler.rs`
- Modify: `apps/cli/src/tui/view_assembler/live_status.rs`
- Modify: `apps/cli/src/tui/view_model/live_status.rs`
- Modify: `apps/cli/src/tui/view_state/run_activity.rs`
- Modify: `apps/cli/src/tui/view_state/run_activity_tests.rs`

- [ ] **Step 1: 编写摘要策略失败测试**

使用固定 `now`，覆盖：Main root 总计时、phase 计时、User audience leaf；Sub 不覆盖 Main；三个并行 Tool 聚合成 `Running 3 tools`；500ms 前不显示 leaf；显示后 750ms 内不轮播；Hook/retry/Diagnostic 不进入默认摘要；Interaction waiting 暂停 active elapsed；stale snapshot 隐藏状态行。

- [ ] **Step 2: 运行 assembler 测试确认失败**

Run: `cargo test -p cli activity_summary -- --nocapture`  
Expected: FAIL。

- [ ] **Step 3: 实现纯摘要选择并切换 LiveStatus**

`ActivitySummaryAssembler::assemble(model, view_state, now)` 返回纯 `ActivitySummaryView`。文案映射集中在该文件；`LiveStatusAssembler` 不再 match `TuiRunStatus`。`RunActivityState` 删除 verb/phase/run identity，只保留 spinner frame、最近 observation monotonic anchor 和 detail 驻留选择状态。

- [ ] **Step 4: 运行 ViewAssembler/ViewState 测试**

Run: `cargo test -p cli activity_summary -- --nocapture && cargo test -p cli live_status -- --nocapture && cargo test -p cli run_activity -- --nocapture`  
Expected: PASS。

- [ ] **Step 5: 提交低噪声展示**

```bash
git add apps/cli/src/tui/view_assembler apps/cli/src/tui/view_model apps/cli/src/tui/view_state
git commit -m "feat(tui): #742 以 Activity 派生低噪声状态"
```

---

### Task 11: 退役旧 Run timing 与分散活动状态源

**Files:**
- Modify: `packages/sdk/src/chat_event.rs`
- Modify: `agent/features/runtime/src/domain/agent_run/{event.rs,domain.rs,tests.rs}`
- Modify: `agent/features/runtime/src/adapters/sdk_event_mapper.rs`
- Modify: `apps/cli/src/tui/adapter/tui_runtime_event.rs`
- Modify: `apps/cli/src/tui/model/conversation/{run_state.rs,model.rs}`
- Modify: `apps/cli/src/tui/view_assembler/live_status.rs`
- Modify/Delete: 旧 `RunTimingView` / `TuiRunTiming` / timing observation 测试和业务 `RunActivityState` 字段
- Modify: `apps/cli/src/tui/architecture_tests.rs`

- [ ] **Step 1: 先扩展架构失败测试**

禁止生产 TUI 出现 `RunTimingView`、`TuiRunTiming`、`timing_observation_revision`、由 `TuiRunStatus` 生成 phase_text、业务 `RunActivityState.verb`、`SpinnerPhase`、`chat_active`、`running_tool_count`。禁止 Tool/Hook/Compact/内容 event 调 Activity Model mutation。

- [ ] **Step 2: 运行架构测试确认旧路径仍被捕获**

Run: `cargo test -p cli tui::architecture_tests -- --nocapture`  
Expected: FAIL，并列出当前待删路径。

- [ ] **Step 3: 物理删除旧链路**

`RunTransitioned` 只保留 run_id/parent/status；删除 `RunTimingSnapshot` 的展示职责和 SDK/TUI timing DTO。Run 仍保留领域 timeout/started time，不删除业务所需时钟。LiveStatus 只读 Activity summary；旧专项事件继续更新内容/ToolCall/Hook notice/compact gauge，但不影响当前 Activity、计时或 spinner 可见性。

- [ ] **Step 4: 运行 Runtime/SDK/TUI 定向测试与 grep 证明**

Run:

```bash
cargo test -p runtime agent_run -- --nocapture
cargo test -p sdk chat_event -- --nocapture
cargo test -p cli tui::architecture_tests -- --nocapture
rg 'RunTimingView|TuiRunTiming|SpinnerPhase|chat_active|running_tool_count|timing_observation_revision' agent/features/runtime/src packages/sdk/src apps/cli/src/tui
```

Expected: 测试 PASS；`rg` 只允许架构测试中的 forbidden token 常量，生产代码零匹配。

- [ ] **Step 5: 提交旧链退役**

```bash
git add agent/features/runtime/src packages/sdk/src apps/cli/src/tui
git commit -m "refactor(runtime,tui): #742 退役旧活动展示协议"
```

---

### Task 12: 补全跨层场景、Guard 与日志诊断

**Files:**
- Modify: `apps/cli/src/tui/app/scenario_tests/chat.rs`
- Modify: `apps/cli/src/tui/architecture_tests.rs`
- Modify: Runtime/TUI 现有 logging tests
- Modify: `.agents/hooks/check-architecture-guards.sh`
- Modify: `.agents/architecture-guard-registry.json`
- Modify: `docs/design/03-engineering/01-architecture-guards.md`

- [ ] **Step 1: 编写失败的完整场景测试**

单场景按 Context→Model→Tool parallel→Hook block→Compact→Interaction→terminal 推送事件，逐帧断言状态行只显示一个稳定摘要、不出现 Hook attempt/retry/token delta、不被 Sub 覆盖，terminal 后无 spinner。另加 revision gap→snapshot 恢复场景。

- [ ] **Step 2: 运行场景测试确认失败**

Run: `cargo test -p cli tui::app::scenario_tests::chat activity -- --nocapture`  
Expected: FAIL，直到全链闭合。

- [ ] **Step 3: 完成场景所需最小接线并扩 Guard**

Guard 锁定：ActivityObservation 只能由 Coordinator 构造；TUI Activity mutation 只在 root reducer；LiveStatus 不依赖 RunStatus；旧字段零生产引用。日志记录 run/activity/source/kind/state/revision/timing，禁止 raw args/stdout/response。

- [ ] **Step 4: 运行场景和 Guard**

Run:

```bash
cargo test -p cli tui::app::scenario_tests::chat -- --nocapture
bash .agents/hooks/check-architecture-guards.sh
```

Expected: PASS。

- [ ] **Step 5: 提交场景与 Guard**

```bash
git add apps/cli/src/tui .agents docs/design/03-engineering/01-architecture-guards.md
git commit -m "test(activity): #742 锁定统一观测链路"
```

---

### Task 13: 同步全部 `docs/design/` Target 与迁移治理

**Files:**
- Modify: `docs/design/01-system/02-ubiquitous-language.md`
- Modify: `docs/design/01-system/03-context-map.md`
- Modify: `docs/design/02-modules/runtime/01-domain-model.md`
- Modify: `docs/design/02-modules/runtime/03-loop-and-state-machine.md`
- Modify: `docs/design/02-modules/runtime/06-ports-and-adapters.md`
- Modify: `docs/design/02-modules/runtime/07-runtime-ownership-and-assembly.md`
- Modify: `docs/design/02-modules/tools/02-ports-and-lifecycle.md`
- Modify: `docs/design/02-modules/hook/01-run-loop-integration.md`
- Modify: `docs/design/02-modules/tui/01-architecture-and-dataflow.md`
- Modify: `docs/design/02-modules/tui/02-model.md`
- Modify: `docs/design/02-modules/tui/03-event-flow-and-acl.md`
- Modify: `docs/design/02-modules/tui/04-view-layer.md`
- Modify: `docs/design/03-engineering/03-migration-governance.md`

- [ ] **Step 1: 生成文档差异检查清单**

逐文件核对设计 spec §10：术语、Context Map、聚合关系、原子发布、Factory 所有权、Tool/Hook 边界、TUI Model/ACL/摘要/ViewState、Current→Target 退出条件。Target 文档不得记录实施进度或提交号。

- [ ] **Step 2: 更新 Target 文档**

统一写入：

```text
Run / RunStep / ToolCall = 业务真相
ActivityObservation     = Runtime application 统一运行观测
SDK Activity PL         = 完整增量 + 快照
TUI                     = 完整事实镜像 + 低噪声摘要
```

明确 Activity 不是聚合根/领域实体，不反向修改业务对象；Runtime 不发布 UI visible/颜色/布局。

- [ ] **Step 3: 更新 Migration Governance**

只记录 Current→Target 状态、责任和退出条件：旧 `RunTimingView`、RunStatus-driven phase、业务 `RunActivityState`、Tool/Hook/Compact 专项 spinner 入口、第二状态源全部退役；逐项填写实际完成证据，不复制 Target 设计。

- [ ] **Step 4: 检查文档链接和外部追踪号规则**

Run:

```bash
rg 'ActivityObservation|ActivityCoordinator|ActivitySnapshot' docs/design
rg '#742|PR #|Issue #' docs/design/01-system docs/design/02-modules
bash .agents/hooks/check-architecture-guards.sh
```

Expected: Activity 目标概念覆盖全部指定文档；新增正文不引用外部追踪号；Guard PASS。

- [ ] **Step 5: 提交设计同步**

```bash
git add docs/design
git commit -m "docs(design): #742 同步 Runtime Activity 架构"
```

---

### Task 14: 完整验证、审查与交付

**Files:**
- Verify only unless failures require scoped fixes

- [ ] **Step 1: 运行格式化检查**

Run: `cargo fmt --all -- --check`  
Expected: PASS。若失败，运行 `cargo fmt --all`，仅提交 rustfmt 结果。

- [ ] **Step 2: 运行分层 crate 测试**

Run:

```bash
cargo test -p sdk
cargo test -p runtime
cargo test -p composition --tests
cargo test -p cli
```

Expected: 全部 PASS，0 failure。

- [ ] **Step 3: 运行静态检查与架构门禁**

Run:

```bash
cargo clippy --workspace --all-targets -- -D warnings
bash .agents/hooks/check-architecture-guards.sh
```

Expected: PASS，0 warning/error。

- [ ] **Step 4: 运行死代码与旧链审计**

Run:

```bash
rg 'RunTimingView|TuiRunTiming|SpinnerPhase|chat_active|running_tool_count|timing_observation_revision' agent packages apps/cli/src/tui
rg 'ActivityObservation \{' agent/features/runtime/src --glob '*.rs'
rg 'observe_activity|replace_activity_snapshot' apps/cli/src/tui --glob '*.rs'
git diff --check origin/main...HEAD
```

Expected:
- 旧 token 只在 guard forbidden 列表或迁移文档中出现；
- `ActivityObservation` 生产构造只在 ActivityCoordinator；
- TUI Activity mutation 生产调用只在 root reducer；
- diff check PASS。

- [ ] **Step 5: 请求代码审查并处理 finding**

调用 `superpowers:requesting-code-review`，要求按设计 spec、跨层测试完整性、第二状态源、默认 UI 噪声和 `docs/design/` 一致性审查。所有 blocker/important finding 修复后重新运行 Step 1–4。

- [ ] **Step 6: 提交最终验证修复**

若有修复：

```bash
git add <实际修复路径>
git commit -m "fix(activity): #742 收口统一观测验收"
```

若无修复，不创建空提交。

- [ ] **Step 7: 推送现有分支并更新 PR**

Run:

```bash
git push origin feature/742-run-status-single-source
gh pr view 1474 --json url,state,headRefName,statusCheckRollup
```

Expected: push 成功；PR #1474 指向当前分支，检查已启动或通过。

---

## 实施纪律

1. 每个 Task 必须测试先行；跨层 Task 不能只依赖最终场景。
2. Activity 失败不得回滚业务结果；但测试必须证明 snapshot 可修复消费者。
3. 不建立 `ActivityPort`、fat observer 或由 Tool/Provider 持有的 callback；Coordinator 是 Runtime application owner。
4. 不在 Activity PL 放 TUI 文案、布局、颜色、visible 或 raw payload。
5. 不保留兼容 fallback 到旧 spinner；迁移完成后旧链必须物理删除。
6. `docs/design/` 与代码同一交付；Target 和 Current→Target 信息不得混写。
7. 每个提交完成后保持工作树可编译、定向测试通过。
