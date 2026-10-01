# 反思进入 loop 状态机与运行态可见性（#1796）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 Memory 反思从裸 `await` 提升为 loop 状态机内可观察执行单元：三触发进入 `RunStatus::Reflecting` 并发布 Reflection Activity，Manual 触发走真 Run（purpose=Reflection），TUI 全程可见。

**Architecture:** 对齐 idle `/compact` 的既有范式——Manual 触发经 `IdleResult::ManualReflectionRequested` 走真 Run（`RunIntent::ManualReflection` + `RunSpec::manual_reflection()`），engine 内 `execute_manual_reflection` 同构 `execute_manual_compaction`；Interval 执行点从 port `classify_terminal` 上移到 engine（拍板 1A）；`ReflectionCompleted` 经 `reflection_return_status` 字段回到进入前状态（拍板 2A，先例 `hard_pause_resume_status`）。可见性经 Activity 快照（不新增 `RuntimeStreamEvent` 变体）。

**Tech Stack:** Rust workspace（`runtime` / `sdk` / `cli`），tokio，TDD（cargo test）。

**父 issue:** #1795（交付单元 1/2）｜ **milestone:** v0.1.0 — Context Engineering

---

## 关键现状事实（已核实，执行时以此为准）

| 事实 | 位置 |
|---|---|
| Manual 裸 await | `session_driver/run_launch.rs:352` `PendingCommand::ReflectNow` 分支（非 issue 写的 `run_launch.rs:353`） |
| Interval 裸 await | `main_run_port.rs:643` `ChatModelObserver::classify_terminal` 内，Run 此时为 `InvokingModel` 态（`ModelInvoked` 转移在 `step_driver.rs:265` 才发生） |
| PreCompact 裸 await | `main_run_port.rs:257` `ChatCompactionObserver::on_compacted`，由 `compaction.rs:148` 在 engine compaction phase 内回调，Run 为 `Compacting` 态 |
| 缺陷 4 已部分修复 | `reflection.rs:128-141` `manual_completion_text` 对 Cancelled/TimedOut 已有独立文案且 `is_error=false`；PR 中披露此偏差 |
| Manual Run 化模板 | `engine/manual_compaction.rs`（`begin_manual_compaction` → activity → 执行 → `CompactionCompleted` → drain `EmptyAndSealed` 收口），无 RunStep、不调模型、不落盘 |
| `begin_manual_compaction` | `domain.rs:615`：前置 intent+status 检查后走正式转移矩阵 |
| Activity 根 purpose 硬编码 | `application/activity/model.rs:123` `ActivityDetail::Run::to_sdk` 硬编码 `purpose: Main`（`Derived` 从未产出）；root 由 `run_events.rs:50` `ensure_run_root` 创建，coordinator 由 `context_factory.rs:315` 构造（不持 intent） |
| 转移表 | `domain.rs:322-375`；`Compacting`/`Completed` 等无 RunPhase activity（`run_events.rs:132` `to_phase` 返回 `None`），Compaction 期间可见性靠 Compaction activity——Reflection 同构 |
| `record_successful_usage` | `application/model/invocation.rs:485`（当前私有），经 `UsageRecordFactory` 构造 `UsageRecordData` 走 `UsageSink::try_record`；`UsageRecordContext.run_step_id` 为必填 |
| `should_run_turn_reflection` | `chat/reflection.rs:248` 纯函数；五入参 engine 侧均可得（`token_usage.stop_reason` 见 `step_driver.rs:250`） |
| `ReflectionTaskAdapter` | `application/reflection/task.rs`：`run_complete` 返回 `ReflectionRunOutcome`（`Completed(status)` / `DisabledSkipped`），内部已处理 cancel/timeout select |
| gate 二次触发 | 原计划作为 #1797（P2）范围排除；实际已在本交付收口：`RunExecutionState` 以 Run 级一次性闸门保证 Interval 反思至多开始一次，未命中不消耗，命中后即使失败/取消也不回滚；对应回归测试覆盖同 Run 多个 `ModelStep::Complete` 与 miss-then-hit。 |

## 文件地图

**新增：**
- `agent/features/runtime/src/application/loop_engine/engine/manual_reflection.rs` — Manual Reflection Run 执行阶段（同构 `manual_compaction.rs`）
- `agent/features/runtime/src/application/loop_engine/engine/reflection.rs` — 三触发共用的 reflection phase（状态转移 + activity + 端口调用 + 收口）

**修改（runtime）：**
- `domain/agent_run/state.rs` — `RunStatus::Reflecting`、`RunTransition::{BeginReflection, ReflectionCompleted}`、`RunTransitionReason::{BeginReflection, ReflectionCompleted, ManualReflectionSettled}`
- `domain/agent_run/domain.rs` — `reflection_return_status` 字段、转移表、`begin_manual_reflection()`
- `domain/agent_run/spec.rs` — `RunIntent::ManualReflection`、`RunSpec::manual_reflection()`
- `application/activity/model.rs` — `ActivitySource::Reflection`、`ActivityKind::Reflection`、`ActivityDetail::{Run{purpose}, Reflection{trigger}}`、`RunPurpose` 枚举
- `application/activity/coordinator.rs` / `runtime_work.rs` — `start_reflection` / `start_manual_reflection`
- `application/activity/run_events.rs` — `to_phase`/`terminal_for_status` 加 `Reflecting => None`；`ensure_run_root` 用 coordinator 持有的 purpose
- `application/loop_engine/engine/contracts.rs` — `ManualReflectionPort`、`ReflectionPhasePort` trait
- `application/loop_engine/run_loop.rs` — `bind_manual_reflection`/`manual_reflection_mut`、`reflection_mut`、`start_reflection_activity`
- `application/loop_engine/engine.rs` — ManualReflection 分支
- `application/loop_engine/engine/step_driver.rs` — Interval 插入点（`ModelInvoked` 后）+ PreCompact 插入点（两处 compact Ready 后）
- `application/loop_engine/run_services.rs` — `RuntimeReflection` 生产服务（含裁决 1 记账）
- `application/loop_engine/chat/main_run_port.rs` — `classify_terminal` 删反思执行；`ChatCompactionObserver` 改为暂存材料；新增 `ChatManualReflection`
- `application/loop_engine/chat/reflection.rs` — `run_interval_reflection` 等编排函数调整（执行材料移交 engine 路径）
- `application/loop_engine/chat/session_driver/run_launch.rs` — `manual_reflection_requested` 标志位、`IdleResult::ManualReflectionRequested` 处理、装配、`run_count`/`RunChanged` 跳过
- `application/loop_engine/chat/idle_lifecycle.rs` — `IdleResult::ManualReflectionRequested`
- `application/model/invocation.rs` — `record_successful_usage` 提为 `pub(crate)`
- `application/run/context_factory.rs` — `ActivityCoordinator::production` 传 purpose
- `application/run/execution_state.rs` — Run 级 Interval 反思一次性闸门，跨 `begin_step` 保持且在 phase 开始时消耗

**修改（sdk / cli）：**
- `packages/sdk/src/activity.rs` — `ActivitySourceView::Reflection`、`ActivityKindView::Reflection`、`ActivityDetailView::Reflection{trigger}`、`RunPurposeView::Reflection`、`ReflectionTriggerView`
- `apps/cli/src/tui/adapter/tui_runtime_event.rs` — `TuiActivityKind::Reflection`、`TuiActivityDetail::Reflection`、`TuiRunPurpose::Reflection`
- `apps/cli/src/tui/adapter/`（event mapping）— sdk→TUI 映射
- `apps/cli/src/tui/view_assembler/activity_summary.rs` — `is_live_main_root` 放宽、`phase_label` 加 `"Reflecting…"`

**文档：**
- `docs/design/02-modules/memory/03-reflection.md` — §1 职责边界补「状态发布」、§4 补 Run 归属与可见性
- `specs/3.3-tui-cli.md` — Reflection activity 展示规范

---

## 设计决策（父 issue 裁决 + 会话拍板，执行 MUST 遵守）

1. **拍板 1A**：Interval 反思执行点上移 engine。`classify_terminal` 回归纯分类；engine 在 `ModelInvoked` 转移后、进入 `Complete` 分支前判定并执行。
2. **拍板 2A**：Run 新增 `reflection_return_status: Option<RunStatus>`，`BeginReflection` 记录、`ReflectionCompleted` 读出并清零。
3. **裁决 1（Usage 计入）**：仅成功 terminal 记账，复用 `record_successful_usage`；失败/取消/DisabledSkipped 不记。**Manual Reflection Run 无 RunStep，`run_step_id` 用 `RunStepId::new_v7()` 生成仅记账 id（PR 披露）；`model_invocation_id` 同理。**
4. **裁决 2（不落盘）**：Manual Reflection Run 无 Step → 无 `append_and_persist`；L2 断言 `message_count`/`updated_at` 不变。
5. **转移 gate**：`(DrainingInput, BeginReflection)` 仅 `ManualReflection` intent；`(ApplyingResponse, BeginReflection)` 与 `(Compacting, BeginReflection)` 仅 `Conversation` intent（防 Manual Reflection Run 误入）。
6. **目的映射**：`Conversation → Main`、`ManualCompaction → Main`（保持现状）、`ManualReflection → Reflection`。

---

### Task 1: 域模型——`Reflecting` 状态与转移矩阵

**Files:**
- Modify: `agent/features/runtime/src/domain/agent_run/state.rs`
- Modify: `agent/features/runtime/src/domain/agent_run/domain.rs`
- Test: `agent/features/runtime/src/domain/agent_run/tests.rs`

- [ ] **Step 1: 写失败测试（L1 转移矩阵）**

在 `tests.rs` 追加：

```rust
#[test]
fn reflecting_is_not_terminal() {
    assert!(!RunStatus::Reflecting.is_terminal());
}

#[test]
fn conversation_run_enters_reflecting_from_applying_response_and_returns() {
    // Arrange：Conversation Run 推进到 ApplyingResponse（经正常 main run 路径）
    let mut run = Run::main_for_test();
    run.start_draining().unwrap();
    advance_run_to_applying_response(&mut run); // 复用 tests.rs 既有推进辅助（若无则按既有测试模式补）
    // Act
    run.transition(RunTransition::BeginReflection).unwrap();
    assert_eq!(run.status(), RunStatus::Reflecting);
    run.transition(RunTransition::ReflectionCompleted).unwrap();
    // Assert：回到进入前状态
    assert_eq!(run.status(), RunStatus::ApplyingResponse);
}

#[test]
fn conversation_run_enters_reflecting_from_compacting_and_returns() {
    let mut run = Run::main_for_test();
    run.start_draining().unwrap();
    advance_run_to_compacting(&mut run);
    run.transition(RunTransition::BeginReflection).unwrap();
    assert_eq!(run.status(), RunStatus::Reflecting);
    run.transition(RunTransition::ReflectionCompleted).unwrap();
    assert_eq!(run.status(), RunStatus::Compacting);
}

#[test]
fn reflection_completed_without_prior_begin_is_rejected() {
    let mut run = Run::main_for_test();
    run.start_draining().unwrap();
    let error = run.transition(RunTransition::ReflectionCompleted).unwrap_err();
    assert!(matches!(error, RunTransitionError::IllegalTransition { .. }));
}

#[test]
fn begin_reflection_from_draining_input_rejected_for_conversation_run() {
    let mut run = Run::main_for_test();
    run.start_draining().unwrap();
    let error = run.transition(RunTransition::BeginReflection).unwrap_err();
    assert!(matches!(error, RunTransitionError::IllegalTransition { .. }));
}

#[test]
fn manual_reflection_run_enters_reflecting_from_draining_input() {
    let mut run = Run::manual_reflection_for_test(); // Task 2 提供；此处先引用
    run.start_draining().unwrap();
    run.begin_manual_reflection().unwrap();
    assert_eq!(run.status(), RunStatus::Reflecting);
    run.transition(RunTransition::ReflectionCompleted).unwrap();
    assert_eq!(run.status(), RunStatus::DrainingInput);
}

#[test]
fn begin_manual_reflection_rejected_for_wrong_intent() {
    let mut run = Run::main_for_test();
    run.start_draining().unwrap();
    assert!(run.begin_manual_reflection().is_err());
}
```

注意：`advance_run_to_applying_response` / `advance_run_to_compacting` 若 tests.rs 无既有辅助，参照既有测试的推进序列（`StartDraining → DrainInputs → ContextPrepared → ModelInvoked` 需先 `run.steps` 有 active step 且记录 invocation——照抄既有 `ModelInvoked` 相关测试的搭建）。`Run::main_for_test` / `manual_reflection_for_test` 若不存在，按 tests.rs 既有构造方式命名对齐。

- [ ] **Step 2: 运行测试确认失败**

```bash
cargo test -p runtime --lib domain::agent_run::tests::reflecting -- --nocapture
```

预期：编译错误（`RunStatus::Reflecting` 未定义）。

- [ ] **Step 3: 实现状态与转移**

`state.rs`：

```rust
pub enum RunStatus {
    // …既有 14 态…
    Compacting,
    Reflecting,   // 新增：与 Compacting 同位（Run 内 LLM 阶段态）
    // …
}

pub enum RunTransition {
    // …
    BeginReflection,
    ReflectionCompleted,
}

pub enum RunTransitionReason {
    // …
    BeginReflection,
    ReflectionCompleted,
    ManualReflectionSettled,
}
// From<RunTransition> 补两臂：BeginReflection→BeginReflection、ReflectionCompleted→ReflectionCompleted
```

`domain.rs`：

```rust
// Run struct 新增字段：
pub(super) reflection_return_status: Option<RunStatus>,

// transition() 内、match 之前（与 ModelInvoked 前置检查同位）：
if transition == RunTransition::BeginReflection {
    let allowed = match self.status {
        RunStatus::DrainingInput => self.spec.intent() == RunIntent::ManualReflection,
        RunStatus::ApplyingResponse | RunStatus::Compacting => {
            self.spec.intent() == RunIntent::Conversation
        }
        _ => false,
    };
    if !allowed {
        log::warn!(target: crate::LOG_TARGET, "run state transition rejected: run_id={} intent={:?} requested_transition={:?} 反思入口状态或意图不合法", self.id, self.spec.intent(), transition);
        return Err(RunTransitionError::IllegalTransition { from: self.status, transition });
    }
    self.reflection_return_status = Some(self.status);
}

// match 新增两臂：
(RunStatus::DrainingInput, RunTransition::BeginReflection)
| (RunStatus::ApplyingResponse, RunTransition::BeginReflection)
| (RunStatus::Compacting, RunTransition::BeginReflection) => RunStatus::Reflecting,
(RunStatus::Reflecting, RunTransition::ReflectionCompleted) => self
    .reflection_return_status
    .take()
    .expect("BeginReflection 转移时已记录 reflection_return_status"),

// begin_manual_reflection（对齐 begin_manual_compaction）：
pub fn begin_manual_reflection(&mut self) -> Result<(), RunTransitionError> {
    if self.spec.intent() != RunIntent::ManualReflection
        || self.status != RunStatus::DrainingInput
    {
        log::warn!(target: crate::LOG_TARGET, "manual reflection command rejected: run_id={} intent={:?} status={:?} 仅手动反思 Run 可从排空阶段进入反思", self.id, self.spec.intent(), self.status);
        return Err(RunTransitionError::IllegalTransition {
            from: self.status,
            transition: RunTransition::BeginReflection,
        });
    }
    self.transition(RunTransition::BeginReflection).map(|_| ())
}
```

注意：`reflection_return_status` 的写入必须在转移成功路径之前（gate 拒绝时不得污染）。`ReflectionCompleted` 经 `ManualReflectionSettled` reason 区分 Manual 收口（对齐 `ManualCompactionSettled`：`(next, transition) == (DrainingInput, ReflectionCompleted)` 时用 `ManualReflectionSettled`）。

- [ ] **Step 4: 运行测试确认通过**

```bash
cargo test -p runtime --lib domain::agent_run
```

- [ ] **Step 5: Commit**

```bash
git add agent/features/runtime/src/domain/agent_run/
git commit -m "feat(runtime): #1796 RunStatus::Reflecting 与反思转移矩阵（reflection_return_status 记忆返回点）"
```

---

### Task 2: `RunIntent::ManualReflection` 与 `RunSpec::manual_reflection()`

**Files:**
- Modify: `agent/features/runtime/src/domain/agent_run/spec.rs`
- Test: `agent/features/runtime/src/domain/agent_run/tests.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn manual_reflection_spec_carries_reflection_intent() {
    assert_eq!(RunSpec::manual_reflection().intent(), RunIntent::ManualReflection);
    // 与 manual_compaction 同构：装配与 main 一致，仅目的不同
    let spec = RunSpec::manual_reflection();
    assert_eq!(spec.name(), RunSpec::main().name());
}
```

- [ ] **Step 2: 运行确认失败**（`manual_reflection` 未定义）

- [ ] **Step 3: 实现**

`spec.rs`：`RunIntent` 新增 `ManualReflection`；新增：

```rust
/// 手动反思 Run：只执行一次 Memory 反思，完成后回到排空阶段收口，
/// **NEVER** 进入模型调用、NEVER 走 ContextPort::build_window。
pub fn manual_reflection() -> Self {
    Self {
        intent: RunIntent::ManualReflection,
        ..Self::full("main", Duration::ZERO)
    }
}
```

同时把 Task 1 引用的 `Run::manual_reflection_for_test`（`#[cfg(test)]` 构造）补上。

- [ ] **Step 4: 运行确认通过**

```bash
cargo test -p runtime --lib domain::agent_run
```

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(runtime): #1796 RunIntent::ManualReflection 与 RunSpec::manual_reflection"
```

---

### Task 3: SDK Activity Published Language 扩展

**Files:**
- Modify: `packages/sdk/src/activity.rs`
- Test: `packages/sdk/src/activity.rs`（既有测试模块，若无则 `packages/sdk/tests/` 对齐既有风格）

- [ ] **Step 1: 写失败测试（序列化往返）**

```rust
#[test]
fn reflection_activity_enums_serde_roundtrip() {
    let kind = ActivityKindView::Reflection;
    let json = serde_json::to_string(&kind).unwrap();
    assert_eq!(serde_json::from_str::<ActivityKindView>(&json).unwrap(), kind);

    let detail = ActivityDetailView::Reflection { trigger: ReflectionTriggerView::Manual };
    let json = serde_json::to_string(&detail).unwrap();
    assert!(json.contains("\"detail_type\":\"reflection\""));
    assert_eq!(serde_json::from_str::<ActivityDetailView>(&json).unwrap(), detail);

    let purpose = RunPurposeView::Reflection;
    let json = serde_json::to_string(&purpose).unwrap();
    assert_eq!(json, "\"reflection\"");

    let source = ActivitySourceView::Reflection(ActivityId::new_v7());
    let json = serde_json::to_string(&source).unwrap();
    assert_eq!(serde_json::from_str::<ActivitySourceView>(&json).unwrap(), source);
}
```

- [ ] **Step 2: 运行确认失败**

- [ ] **Step 3: 实现**

`activity.rs` 新增（全部遵循既有 `snake_case` / `tag` 风格）：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionTriggerView {
    Interval,
    PreCompact,
    Manual,
}

// ActivitySourceView 新增：Reflection(ActivityId),
// ActivityKindView 新增：Reflection,
// ActivityDetailView 新增：Reflection { trigger: ReflectionTriggerView },
// RunPurposeView 新增：Reflection,
```

- [ ] **Step 4: 运行确认通过 + sdk 全量**

```bash
cargo test -p sdk --all-targets
```

注意：sdk 内若有对 `ActivityKindView`/`ActivityDetailView`/`RunPurposeView` 的穷尽 match（如 schema 测试、示例构造），编译器会指出，逐一补臂。

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(sdk): #1796 Activity PL 新增 Reflection kind/detail/source 与 RunPurposeView::Reflection"
```

---

### Task 4: runtime Activity 层——Reflection 发布与 purpose 贯通

**Files:**
- Modify: `agent/features/runtime/src/application/activity/model.rs`
- Modify: `agent/features/runtime/src/application/activity/coordinator.rs`（构造签名加 purpose）
- Modify: `agent/features/runtime/src/application/activity/runtime_work.rs`
- Modify: `agent/features/runtime/src/application/activity/run_events.rs`
- Modify: `agent/features/runtime/src/application/run/context_factory.rs:315`（production 调用点传 purpose）
- Test: `agent/features/runtime/src/application/activity/coordinator_tests.rs`、`run_events_tests.rs`

- [ ] **Step 1: 写失败测试**

```rust
// run_events_tests.rs
#[test]
fn manual_reflection_run_root_activity_carries_reflection_purpose() {
    let coordinator = coordinator_with_purpose(&RunId::new_v7(), RunPurpose::Reflection);
    coordinator.observe_run_events(&[RuntimeLifecycleEvent::Started {
        run_id: coordinator.run_id().clone(),
        parent_run_id: None,
    }]).unwrap();
    let root = /* 取 root activity */;
    assert!(matches!(root.detail, ActivityDetail::Run { purpose: RunPurpose::Reflection }));
}

// coordinator_tests.rs
#[test]
fn reflection_activity_start_and_finish() {
    let (coordinator, _) = coordinator();
    coordinator.ensure_run_observation_started().unwrap();
    let id = coordinator.start_manual_reflection(sdk::ReflectionTriggerView::Manual).unwrap();
    coordinator.finish(id, ActivityTerminal::Succeeded).unwrap();
    // 断言 activity kind/source/detail 与终态
}

#[test]
fn reflecting_and_compacting_like_states_emit_no_run_phase() {
    assert_eq!(super::run_events::to_phase(RunStatus::Reflecting), None);
    assert_eq!(super::run_events::terminal_for_status(RunStatus::Reflecting), None);
}
```

- [ ] **Step 2: 运行确认失败**

- [ ] **Step 3: 实现**

1. `model.rs`：
   - 新增 `pub(crate) enum RunPurpose { Main, Reflection }`，`to_sdk` 映射 `Main→RunPurposeView::Main`、`Reflection→RunPurposeView::Reflection`
   - `ActivityDetail::Run` 改为 `Run { purpose: RunPurpose }`；`to_sdk` 不再硬编码
   - 新增 `ActivitySource::Reflection(ActivityId)`、`ActivityKind::Reflection`、`ActivityDetail::Reflection { trigger: sdk::ReflectionTriggerView }` 及 `to_sdk` 臂
2. `coordinator.rs`：构造持 `run_purpose: RunPurpose`；`production(run_id, publisher, purpose)` / `production_without_publisher(run_id, purpose)`；`ensure_run_root`（run_events.rs:56）`detail: ActivityDetail::Run { purpose: self.run_purpose }`
3. `runtime_work.rs` 新增（对齐 `start_manual_compaction`）：

```rust
pub(crate) fn start_manual_reflection(
    &self,
    trigger: sdk::ReflectionTriggerView,
) -> Result<ActivityId, ActivityError> {
    self.transaction(|| {
        self.ensure_run_observation_started()?;
        let parent_activity_id = self
            .live_run_root_id()
            .ok_or_else(|| ActivityError::UnknownActivity(ActivityId::new("run-activity")))?;
        self.start(StartActivity {
            run_step_id: None,
            parent_activity_id: Some(parent_activity_id),
            source: ActivitySource::Reflection(ActivityId::new_v7()),
            kind: ActivityKind::Reflection,
            detail: ActivityDetail::Reflection { trigger },
            audience: ActivityAudienceView::User,
        })
    })
}

/// Interval / PreCompact：归属当前对话 Run 根下（无独立 RunStep 归属，与 manual compaction 同）。
pub(crate) fn start_reflection(
    &self,
    parent_activity_id: ActivityId,
    trigger: sdk::ReflectionTriggerView,
) -> Result<ActivityId, ActivityError> {
    self.start(StartActivity {
        run_step_id: None,
        parent_activity_id: Some(parent_activity_id),
        source: ActivitySource::Reflection(ActivityId::new_v7()),
        kind: ActivityKind::Reflection,
        detail: ActivityDetail::Reflection { trigger },
        audience: ActivityAudienceView::User,
    })
}
```

4. `run_events.rs`：`to_phase`/`terminal_for_status` 的 `None` 臂加 `RunStatus::Reflecting`
5. `context_factory.rs:315`：`ActivityCoordinator::production(run_id, activity_publisher, purpose)`，purpose 由 `request.spec().intent()` 映射（`ManualReflection→Reflection`，其余 `→Main`）
6. 全部既有 `production_without_publisher` 调用点（测试）补 purpose 参数——编译器列全

- [ ] **Step 4: 运行确认通过**

```bash
cargo test -p runtime --lib application::activity
```

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(runtime): #1796 Activity 层 Reflection 发布与 Run purpose 按 intent 贯通"
```

---

### Task 5: TUI——adapter 映射与 spinner 呈现

**Files:**
- Modify: `apps/cli/src/tui/adapter/tui_runtime_event.rs`
- Modify: `apps/cli/src/tui/adapter/`（sdk→TUI 转换点，编译器定位穷尽 match）
- Modify: `apps/cli/src/tui/view_assembler/activity_summary.rs`
- Test: `apps/cli/src/tui/view_assembler/activity_summary_tests.rs`、`apps/cli/src/tui/adapter/event_mapping_tests.rs`

- [ ] **Step 1: 写失败测试**

```rust
// activity_summary_tests.rs
#[test]
fn reflection_purpose_root_is_live_main_root() {
    // 构造 purpose=Reflection 的 Running root activity → assemble 应返回 Some
}

#[test]
fn reflection_leaf_shows_reflecting_label() {
    // 构造 detail=Reflection{trigger} 的 Running leaf → phase_text == "Reflecting…"
}

// event_mapping_tests.rs
#[test]
fn sdk_reflection_activity_maps_to_tui_reflection() {
    // sdk ActivityView{kind:Reflection, detail:Reflection{trigger:Manual}} → TuiActivityKind::Reflection / TuiActivityDetail::Reflection
}
```

- [ ] **Step 2: 运行确认失败**

- [ ] **Step 3: 实现**

1. `tui_runtime_event.rs`：`TuiActivityKind::Reflection`、`TuiActivityDetail::Reflection { trigger: TuiReflectionTrigger }`、`TuiRunPurpose::Reflection`（含 sdk 转换臂）
2. adapter 转换：sdk `ActivityKindView::Reflection → TuiActivityKind::Reflection` 等，穷尽 match 补臂
3. `activity_summary.rs`：
   - `is_live_main_root`：`purpose: TuiRunPurpose::Main | TuiRunPurpose::Reflection`
   - `phase_label`：`TuiActivityDetail::Reflection { .. } => "Reflecting…"`

- [ ] **Step 4: 运行确认通过**

```bash
cargo test -p cli --lib tui::view_assembler tui::adapter
```

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(tui): #1796 Reflection activity 映射与 spinner 呈现（purpose 放宽 Main|Reflection）"
```

---

### Task 6: engine reflection phase + Interval 上移（拍板 1A）

**Files:**
- Create: `agent/features/runtime/src/application/loop_engine/engine/reflection.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/engine/contracts.rs`（`ReflectionPhasePort`）
- Modify: `agent/features/runtime/src/application/loop_engine/run_loop.rs`（`reflection_mut`、`start_reflection_activity`）
- Modify: `agent/features/runtime/src/application/loop_engine/engine/step_driver.rs`（Interval 插入点）
- Modify: `agent/features/runtime/src/application/loop_engine/engine.rs`（`mod reflection;`）
- Modify: `agent/features/runtime/src/application/loop_engine/chat/main_run_port.rs`（`classify_terminal` 删执行）
- Modify: `agent/features/runtime/src/application/loop_engine/chat/reflection.rs`（保留 `should_run_turn_reflection` 与编排辅助，删掉不再有调用方的函数）
- Test: `agent/features/runtime/src/application/loop_engine/engine_reflection_tests.rs`（新建，对齐 `engine_activity_tests.rs` 风格）、`reflection_trigger_tests.rs`（更新）

- [ ] **Step 1: 写失败测试（L2 模块协作）**

```rust
// engine_reflection_tests.rs：用 engine 既有 fake port 体系
#[tokio::test]
async fn interval_reflection_publishes_begin_and_terminal_activity_under_current_run() {
    // Arrange：fake model 返回 text-only Complete；step_count 命中 interval；
    // reflection fake 返回 Succeeded(changed=0)
    // Act：跑 execute_step
    // Assert：
    // 1. Run 事件序列含 DrainingInput→…→ApplyingResponse→Reflecting→ApplyingResponse
    // 2. activity 快照含恰好一次 Reflection activity（begin Running + terminal Succeeded），
    //    parent = 当前 Run root，trigger = Interval
    // 3. step 正常 ContinueAfterResponse 收口
}

#[tokio::test]
async fn interval_reflection_failure_does_not_kill_run() {
    // reflection fake 返回 Failed → activity Failed，Run 仍走 Complete 正常收口 Completed
}

#[tokio::test]
async fn disabled_reflection_is_noop() {
    // config disabled → 无 BeginReflection 转移、无 Reflection activity
}
```

- [ ] **Step 2: 运行确认失败**

- [ ] **Step 3: 实现**

1. `contracts.rs`：

```rust
/// 反思执行端口：engine 持有状态机与 activity，端口只提供执行能力。
pub trait ReflectionPhasePort: Send {
    async fn run_reflection(
        &mut self,
        trigger: ReflectionTaskTrigger,
        messages: Vec<share::message::Message>,
        run_step_id: Option<&sdk::RunStepId>,
        cancel: CancellationToken,
    ) -> Result<crate::application::reflection::ReflectionRunOutcome, LoopEngineError>;
}
```

2. `engine/reflection.rs`（三触发共用）：

```rust
pub(super) async fn run_reflection_phase(
    run: &mut Run,
    execution: &mut RunExecutionState,
    cancel: &CancellationToken,
    port: &mut RunLoop<'_>,
    trigger: ReflectionTaskTrigger,
    messages: Vec<share::message::Message>,
    run_step_id: Option<&sdk::RunStepId>,
) -> Result<(), LoopEngineError> {
    transition_and_emit(run, execution, port, RunTransition::BeginReflection).await?;
    let activity_id = match port.start_reflection_activity(trigger_to_view(trigger)) {
        Ok(id) => Some(id),
        Err(error) => {
            log::warn!(target: crate::LOG_TARGET, "[run_loop] 无法发布反思 activity，继续执行反思: {error}");
            None
        }
    };
    let step_cancel = /* 由调用方传入的 step cancel */;
    let outcome = port
        .reflection_mut()
        .run_reflection(trigger, messages, run_step_id, step_cancel)
        .await?;
    if let Some(activity_id) = activity_id {
        let terminal = match &outcome {
            ReflectionRunOutcome::DisabledSkipped => ActivityTerminal::Cancelled, // 实际不会到这：判定在 engine 之前
            ReflectionRunOutcome::Completed(c) => match c.status {
                ReflectionTaskCompletionStatus::Succeeded => ActivityTerminal::Succeeded,
                ReflectionTaskCompletionStatus::Failed => ActivityTerminal::Failed,
                ReflectionTaskCompletionStatus::Cancelled => ActivityTerminal::Cancelled,
                ReflectionTaskCompletionStatus::TimedOut => ActivityTerminal::Terminated,
            },
        };
        let _ = port.finish_activity(activity_id, terminal);
    }
    transition_and_emit(run, execution, port, RunTransition::ReflectionCompleted).await?;
    Ok(())
}
```

要点：反思任何 outcome（含 Failed/Cancelled/TimedOut）都 **不终止宿主 Run**——与现状行为一致（`classify_terminal` 内 outcome 不影响 Run）。宿主 Run 的取消由 `handle_interrupt`/`handle_step_control` 既有路径负责（step cancel 触发时 adapter 返回 Cancelled，Run 由 step_driver 既有取消检查收口——需核验：现状 classify 内取消后 Run 走 `ModelInvocationOutcome::Cancelled`？不，反思在 classify 内、模型已成功。上移后反思取消时 step 已完成模型调用，Run 继续 Complete 路径；Esc 终止 Run 是 P2 范围，本批行为对齐现状）。

3. `step_driver.rs` Interval 插入点（`step_driver.rs:264-265` `record_model_invocation` + `ModelInvoked` 转移**之后**、`match model_step` **之前**）：

```rust
// 反思 phase：Interval 触发判定与执行（拍板 1A：执行点上移 engine）。
if matches!(model_step, ModelStep::Complete { .. })
    && crate::application::loop_engine::chat::reflection::should_run_turn_reflection(
        port.reflection_memory_config(), // RunLoop 新增只读访问，或经 reflection port 暴露判定
        execution.step_count(),
        false,
        &token_usage.stop_reason,
        false,
    )
{
    let messages = execution.messages().to_vec();
    run_reflection_phase(
        run, execution, &step_cancel, port,
        ReflectionTaskTrigger::Interval { step_count: execution.step_count() },
        messages,
        Some(&step_id),
    ).await?;
}
```

注意 `should_run_turn_reflection` 的 `before_finish_gate_continue` 入参现状恒为 `false`（`main_run_port.rs:641` 调用点传 `false`），保持。config 获取：`RunLoop` 不持 runtime_context——经 `ReflectionPhasePort` 新增 `fn reflection_memory_config(&self) -> share::config::MemoryConfig`（生产实现从 runtime_context 读）或把判定整个下沉到端口：`async fn interval_reflection_material(&mut self, step_count, stop_reason) -> Option<Vec<Message>>`。**推荐后者**：判定与材料收集都在端口，engine 只问「有没有要做的事」，更符合端口职责：

```rust
pub trait ReflectionPhasePort: Send {
    /// Interval 判定：命中返回待反思消息快照，未命中/禁用返回 None。
    fn interval_reflection_messages(
        &self,
        step_count: usize,
        stop_reason: &provider::ProviderStopReasonData,
        messages: &[share::message::Message],
    ) -> Option<Vec<share::message::Message>>;

    async fn run_reflection(/* 同上 */) -> Result<ReflectionRunOutcome, LoopEngineError>;
}
```

engine 侧：`if let Some(messages) = port.reflection_mut().interval_reflection_messages(execution.step_count(), &token_usage.stop_reason, execution.messages()) { run_reflection_phase(...) }`

4. `main_run_port.rs` `classify_terminal`：删除 `run_interval_reflection` 调用与 `announce_memory_update`（announce 挪到 `RuntimeReflection.run_reflection` 成功后——它持 `event_sink`，行为不变）。`ChatModelObserver` 的 `reflection_tasks` 字段若不再有消费方则删除。
5. `run_services.rs` 新增 `RuntimeReflection`（生产 `ReflectionPhasePort`）：持 `runtime_context` + `reflection_tasks` + `system_prompt` + `language`。`run_reflection` 内：调 `chat::reflection` 的既有 `run` 编排（或 `adapter.run_complete`），成功后 `announce_memory_update`。**裁决 1 记账也在这里（Task 9 接线，本任务先留 `record_reflection_usage` 空挂点或直接完成——见 Task 9 顺序说明）。**
6. `run_loop.rs`：`reflection: Option<&'a mut dyn ReflectionPhasePort>` + `bind_reflection` + `reflection_mut()`；`start_reflection_activity(trigger)`（调 coordinator `start_reflection(parent=self.activity_parent_id()?, trigger)`）。

- [ ] **Step 4: 运行确认通过**

```bash
cargo test -p runtime --lib application::loop_engine
```

既有 `reflection_trigger_tests.rs` / `reflection_notice_tests.rs` 中调用已删函数的用例需改写为 engine 路径或 `RuntimeReflection` 直测——逐一按编译错误处理，**NEVER 静默删除行为断言，改写为等价新路径断言**。

- [ ] **Step 5: Commit**

```bash
git commit -am "refactor(runtime): #1796 Interval 反思执行点上移 engine，Reflecting 态与 Reflection activity 落地"
```

---

### Task 7: PreCompact——engine compact Ready 后插入

**Files:**
- Modify: `agent/features/runtime/src/application/loop_engine/chat/main_run_port.rs`（`ChatCompactionObserver` 改暂存）
- Modify: `agent/features/runtime/src/application/loop_engine/run_services.rs`（`RuntimeReflection` 持暂存槽）
- Modify: `agent/features/runtime/src/application/loop_engine/engine/step_driver.rs`（两处 compact Ready 后插入）
- Test: `agent/features/runtime/src/application/loop_engine/chat/pre_compact_trigger_tests.rs`（更新）、`engine_reflection_tests.rs`（新增场景）

- [ ] **Step 1: 写失败测试（L2）**

```rust
#[tokio::test]
async fn pre_compact_reflection_runs_inside_compacting_state_with_activity() {
    // fake compaction 返回 Committed；reflection fake Succeeded
    // Assert：事件序列含 …BeginCompaction→Compacting→Reflecting→Compacting→CompactionCompleted…
    // activity：Reflection{trigger:PreCompact} begin+terminal 各一次，parent=当前 Run root
}

#[tokio::test]
async fn pre_compact_reflection_skipped_when_compact_skipped() {
    // fake compaction 返回 Skipped → 无 Reflecting、无 Reflection activity（对齐既有 only_runs_on_committed）
}
```

- [ ] **Step 2: 运行确认失败**

- [ ] **Step 3: 实现**

1. 材料暂存槽：`main_run_port.rs` `ChatCompactionObserver` 不再执行反思，`on_compacted` 改为：

```rust
async fn on_compacted(&mut self, outcome, discarded_messages) -> Result<(), LoopEngineError> {
    if matches!(outcome, CompactOutcome::Committed(_)) {
        *self.pending_pre_compact.lock().unwrap() = Some(discarded_messages.to_vec());
    }
    Ok(())
}
```

`pending_pre_compact: Arc<std::sync::Mutex<Option<Vec<Message>>>>` 在 `run_launch.rs` 装配时创建，`ChatCompactionObserver` 与 `RuntimeReflection` 各持一份。

2. `RuntimeReflection` 新增：

```rust
fn take_pre_compact_messages(&self) -> Option<Vec<Message>> {
    self.pending_pre_compact.lock().unwrap().take()
}
```

3. `step_driver.rs` 两处 compact Ready 之后（`:95-111` needs_compaction 路径与 `:183-200` ModelContextExceeded 路径），在 `finish_activity(compact_activity_id, Succeeded)` 之后、`CompactionCompleted` 转移之前插入：

```rust
if let Some(messages) = port.reflection_mut().take_pre_compact_messages() {
    run_reflection_phase(
        run, execution, &step_cancel, port,
        ReflectionTaskTrigger::PreCompact,
        messages,
        Some(&step_id),
    ).await?;
}
```

（`Compacting --BeginReflection--> Reflecting --ReflectionCompleted--> Compacting`，由 Task 1 转移矩阵支撑。）

4. `chat/reflection.rs`：`maybe_run_pre_compact_reflection` / `run_pre_compact_reflection` 删除（无调用方后），`announce_memory_update` 保留（挪至 RuntimeReflection 使用）。

- [ ] **Step 4: 运行确认通过**

```bash
cargo test -p runtime --lib application::loop_engine
```

- [ ] **Step 5: Commit**

```bash
git commit -am "refactor(runtime): #1796 PreCompact 反思进入 Compacting 内的 Reflecting 态"
```

---

### Task 8: Manual 走真 Run（Run 化）

**Files:**
- Modify: `agent/features/runtime/src/application/loop_engine/chat/idle_lifecycle.rs`（`IdleResult::ManualReflectionRequested`）
- Modify: `agent/features/runtime/src/application/loop_engine/chat/session_driver/run_launch.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/engine/contracts.rs`（`ManualReflectionPort`）
- Modify: `agent/features/runtime/src/application/loop_engine/run_loop.rs`（`bind_manual_reflection`）
- Modify: `agent/features/runtime/src/application/loop_engine/engine.rs`（ManualReflection 分支）
- Create: `agent/features/runtime/src/application/loop_engine/engine/manual_reflection.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/main_run_port.rs`（`ChatManualReflection`）
- Test: `agent/features/runtime/src/application/loop_engine/chat/reflection_manual_tests.rs`（重写为 Run 化路径）、`engine_reflection_tests.rs`

- [ ] **Step 1: 写失败测试（L2）**

```rust
#[tokio::test]
async fn manual_reflection_run_lifecycle() {
    // idle 受理 ReflectNow → ManualReflectionRequested → 创建 ManualReflection Run
    // Assert：
    // 1. Run 事件：Created→DrainingInput→Reflecting→DrainingInput→Completed
    // 2. root activity purpose=Reflection；Reflection{trigger:Manual} activity begin+terminal
    // 3. 不发 RunChanged、run_count 不变（通过 session 层事件断言）
    // 4. 无 build_window 调用（fake context port 断言 0 次）
    // 5. 无 append_and_persist（fake persistence 断言 0 次）
    // 6. 终态 CommandResultText 文案为成功三态之一
}

#[tokio::test]
async fn manual_reflection_disabled_is_noop_without_run() {
    // config disabled → outcome DisabledSkipped → 不创建 Run、无 activity、
    // CommandResultText 为「未启用」文案
}
```

Manual 的 DisabledSkipped no-op 门禁在 **idle 受理前**判定（配置关闭时不设置 `manual_reflection_requested`，直接回「未启用」文案）——否则空 Run 闪现。实现位置：`PendingCommand::ReflectNow` 分支内先查 `memory_config`（对齐 `ReflectionDisabledReason::of`），disabled → 直接发 `CommandResultText` + `continue`。

- [ ] **Step 2: 运行确认失败**

- [ ] **Step 3: 实现**

1. `idle_lifecycle.rs`：`IdleResult` 新增 `ManualReflectionRequested`
2. `run_launch.rs`：
   - `let mut manual_reflection_requested = false;`（对齐 compaction 标志位）
   - `PendingCommand::ReflectNow` 分支改为：disabled 判定（发「未启用」文案 continue）→ `manual_reflection_requested = true; continue;`（删除裸 await 整块）
   - idle 分支：`else if manual_reflection_requested { IdleResult::ManualReflectionRequested }`（放在 `manual_compaction_requested` 判定同位）
   - `IdleResult::ManualReflectionRequested => { manual_reflection_requested = false; (ChatId::new_v7().to_string(), Vec::new()) }`
   - `let manual_reflection_run = matches!(idle_result, IdleResult::ManualReflectionRequested);`
   - **`run_count`/`RunChanged` 跳过**（硬约束 3）：

```rust
if !manual_reflection_run {
    run_count += 1;
    sink.send_event(RuntimeStreamEvent::RunChanged(run_count)).await;
}
```

   - `prepare_main_run` 的 spec：`if manual_compaction_run { RunSpec::manual_compaction() } else if manual_reflection_run { RunSpec::manual_reflection() } else { RunSpec::main() }`
   - 装配 `ChatManualReflection` + `loop_context.bind_manual_reflection(&mut manual_reflection)` + `loop_context.bind_reflection(&mut runtime_reflection)`（reflection phase 端口所有 Main Run 都绑，供 Interval/PreCompact 使用）
   - `ChatManualReflection` 的 messages 快照：装配点 `wiring.bind_main_run().await` → `bound.session().structured_messages()`（对齐现 ReflectNow 分支的取法）
3. `contracts.rs`：

```rust
/// 手动反思端口：由会话驱动装配，承载冻结的消息快照并发布用户可见结果。
pub trait ManualReflectionPort: Send {
    async fn run_manual_reflection(
        &mut self,
        run_id: &sdk::RunId,
        cancel: &CancellationToken,
    ) -> Result<ManualReflectionOutcome, LoopEngineError>;
}

pub enum ManualReflectionOutcome {
    /// 反思到达终态（成功或失败），Run 继续收口 Completed。
    Ready(ReflectionTaskCompletionStatus),
    Cancelled,
    TimedOut,
}
```

4. `ChatManualReflection`（`main_run_port.rs`，对齐 `ChatManualCompaction`）：持 `runtime_context` + `reflection_tasks` + `system_prompt` + `language` + `messages: Vec<Message>`。实现内：调 `adapter.run_complete(...)` → 按 outcome 发 `CommandResultText`（复用 `manual_reflection_outcome_text` 三态文案）→ 映射 `ManualReflectionOutcome`。**裁决 1 记账**：成功时经 `RuntimeReflection` 同一路径（见 Task 9，记账统一收在 `RuntimeReflection::record_succeeded_usage`，Manual 端口复用）。
5. `engine/manual_reflection.rs`（同构 `manual_compaction.rs`）：

```rust
pub(super) async fn execute_manual_reflection(
    run: &mut Run,
    execution: &mut RunExecutionState,
    cancel: &CancellationToken,
    port: &mut RunLoop<'_>,
) -> Result<ManualReflectionDirective, LoopEngineError> {
    run.begin_manual_reflection()?;
    emit_events(run, execution, port).await?;
    let activity_id = match port.start_manual_reflection_activity() { /* 同 compaction 容错 */ };
    let Some(manual_reflection) = port.manual_reflection_mut() else {
        return Err(LoopEngineError::Adapter("手动反思 Run 未绑定手动反思端口".to_string()));
    };
    match manual_reflection.run_manual_reflection(run.id(), cancel).await? {
        ManualReflectionOutcome::Ready(status) => {
            if let Some(id) = activity_id {
                let terminal = match status {
                    ReflectionTaskCompletionStatus::Succeeded => ActivityTerminal::Succeeded,
                    _ => ActivityTerminal::Failed,
                };
                let _ = port.finish_activity(id, terminal);
            }
            transition_and_emit(run, execution, port, RunTransition::ReflectionCompleted).await?;
            Ok(ManualReflectionDirective::Settled)
        }
        ManualReflectionOutcome::Cancelled => {
            if let Some(id) = activity_id { let _ = port.finish_activity(id, ActivityTerminal::Cancelled); }
            terminate_interrupted_run(run, execution, port).await?;
            Ok(ManualReflectionDirective::Terminal)
        }
        ManualReflectionOutcome::TimedOut => {
            if let Some(id) = activity_id { let _ = port.finish_activity(id, ActivityTerminal::Terminated); }
            timeout_run(run, execution, port).await?;
            Ok(ManualReflectionDirective::Terminal)
        }
    }
}
```

6. `engine.rs` `run_loop_body`（`ManualCompaction` 分支之后）：

```rust
if run.spec().intent() == RunIntent::ManualReflection
    && matches!(execute_manual_reflection(run, execution, cancel, port).await?, ManualReflectionDirective::Terminal)
{
    return Ok(LoopDirective::Terminal);
}
```

7. `run_loop.rs`：`start_manual_reflection_activity()` → coordinator `start_manual_reflection(ReflectionTriggerView::Manual)`
8. `chat/reflection.rs`：`run_manual_reflection` 删除（编排迁入 `ChatManualReflection`）；`manual_reflection_outcome_text` 保留

- [ ] **Step 4: 运行确认通过**

```bash
cargo test -p runtime --lib application::loop_engine
```

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(runtime): #1796 /reflect-now 走真 Run（ManualReflection intent + Reflecting 态 + purpose=Reflection）"
```

---

### Task 9: 裁决 1——反思 Usage 计入 `/usage`

**Files:**
- Modify: `agent/features/runtime/src/application/model/invocation.rs`（`record_successful_usage` 提 `pub(crate)`）
- Modify: `agent/features/runtime/src/application/loop_engine/run_services.rs`（`RuntimeReflection` 记账）
- Test: `agent/features/runtime/src/application/loop_engine/engine_reflection_tests.rs`、`application/model/invocation_usage_tests.rs`（不动）

- [ ] **Step 1: 写失败测试（L2）**

```rust
#[tokio::test]
async fn succeeded_reflection_records_usage_via_shared_path() {
    // reflection fake 返回 Succeeded(metadata: input=100, output=50)
    // Assert：UsageSink（fake）收到恰好 1 条 UsageRecordData，
    //   input_tokens=100 output_tokens=50，run_id 为当前 Run，model 正确
}

#[tokio::test]
async fn failed_and_cancelled_reflection_record_no_usage() {
    // Failed / Cancelled 各跑一次 → UsageSink 0 条
}

#[tokio::test]
async fn disabled_reflection_records_no_usage() {
    // DisabledSkipped → 0 条
}
```

- [ ] **Step 2: 运行确认失败**

- [ ] **Step 3: 实现**

1. `invocation.rs:485`：`fn record_successful_usage` → `pub(crate) fn record_successful_usage`
2. `RuntimeReflection`（run_services.rs）新增私有方法：

```rust
fn record_succeeded_usage(
    &self,
    run_id: &sdk::RunId,
    run_step_id: Option<&sdk::RunStepId>,
    metadata: &ReflectionTaskMetadata,
) {
    let usage = provider::RawUsageSnapshotData {
        input_tokens: Some(metadata.input_tokens),
        output_tokens: Some(metadata.output_tokens),
        ..Default::default()
    };
    crate::application::model::invocation::record_successful_usage(
        self.runtime_context.usage_sink().as_ref(),
        crate::application::model::usage::UsageRecordContext {
            session_id: sdk::SessionId::new(self.runtime_context.skill_load_session_id()),
            run_id: run_id.clone(),
            // Manual Reflection Run 无 RunStep：生成仅记账 id（PR 披露）。
            run_step_id: run_step_id.cloned().unwrap_or_else(sdk::RunStepId::new_v7),
            model_invocation_id: sdk::ModelInvocationId::new_v7(),
            model: self.runtime_context.provider_ref().model.clone(),
        },
        // record_successful_usage 现签名吃 &InvocationResponse——
        // 需先把签名重构为吃 RawUsageSnapshotData（见下），再调。
        ...
    );
}
```

**签名重构（DRY 前提）**：`record_successful_usage` 现签名 `(sink, context, response: &InvocationResponse, clock)`，函数体只用 `response.usage`。重构为 `(sink, context, usage: RawUsageSnapshotData, clock)`，`invocation.rs:321` 调用点改传 `response.usage.clone()`。`UsageRecordFactory::build_from_raw_usage` 本就吃 `RawUsageSnapshotData`，改动极小。`invocation_usage_tests.rs` 同步更新。

3. `run_reflection`（Task 6 的 `ReflectionPhasePort::run_reflection` 实现）成功分支：

```rust
if let ReflectionRunOutcome::Completed(completion) = &outcome {
    if completion.status == ReflectionTaskCompletionStatus::Succeeded {
        if let Some(metadata) = &completion.metadata {
            self.record_succeeded_usage(run_id, run_step_id, metadata);
        }
    }
}
```

`run_reflection` 签名需带 `run_id: &sdk::RunId`（Task 6 补上）。`ChatManualReflection` 成功路径同样调用（经共享的 `RuntimeReflection::record_succeeded_usage`，`run_step_id=None`）。

注意：`ReflectionTaskMetadata.input_tokens` 为 `u32`，失败路径恒 0；`was_reported` 要求至少一个 `Some`——`Some(0)` 也会记账，但失败路径不调用故无影响；成功路径 token 为 0（理论上）会记 0 值记录，属如实反映。

- [ ] **Step 4: 运行确认通过 + 手工数字对比**

```bash
cargo test -p runtime --lib application
```

并记录：本改动使 `/usage` 新增反思 Run 的 token 消耗（实施前后对比数字写入 PR Test plan）。

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(runtime): #1796 裁决1 反思成功 terminal 复用 record_successful_usage 计入 /usage"
```

---

### Task 10: 裁决 2（不落盘）与 DisabledSkipped 断言补强

**Files:**
- Test: `agent/features/runtime/src/application/loop_engine/chat/session_driver_session_lifecycle_tests.rs` 或 `engine_reflection_tests.rs`

- [ ] **Step 1: 写测试（L2）**

```rust
#[tokio::test]
async fn manual_reflection_run_does_not_persist_session_or_touch_updated_at() {
    // fake session management：跑 Manual Reflection Run 前后
    // Assert：session_management 落盘调用 0 次；SessionSummary.message_count 不变；updated_at 不变
}

#[tokio::test]
async fn disabled_manual_reflection_creates_no_run_no_activity_no_notice() {
    // config disabled → /reflect-now → 无 Run 事件、无 activity、无 usage、
    // 仅一条「未启用」CommandResultText（is_error=false）
}
```

**核验点（实施时必做）**：`updated_at` 的刷新来源——核查 `session_state.update_session` / `SessionSummary` 生成链路，确认 Manual Reflection Run 路径（`prepare_main_run`/`create_main_run`/launcher）不触碰；若发现 `bind_main_run` 或 Run 创建有刷新副作用，定位并豁免 ManualReflection intent，同时在 PR 披露。

- [ ] **Step 2-4: 运行→修复→通过→Commit**

```bash
cargo test -p runtime --lib application
git commit -am "test(runtime): #1796 裁决2 Manual Reflection Run 不落盘不刷新 updated_at + DisabledSkipped no-op 断言"
```

---

### Task 11: 终态三态文案核验与既有测试改写

**Files:**
- Modify: `agent/features/runtime/src/application/loop_engine/chat/reflection.rs`（`manual_reflection_outcome_text`，如需）
- Test: `reflection_manual_tests.rs`、`reflection_notice_tests.rs`

- [ ] **Step 1: 核验 + 失败测试**

核验现状（`reflection.rs:128-141`）：Cancelled/TimedOut 已有独立文案且 `is_error=false`。**与 issue 缺陷 4 描述不符——以代码为准，PR 披露。** 补三态判定测试：

```rust
#[test]
fn manual_outcome_text_maps_three_terminal_states() {
    // Succeeded(changed>0) → 更新文案, is_error=false
    // Succeeded(0) → 无变更文案, is_error=false
    // Failed → 失败文案, is_error=true
    // Cancelled → 「Reflection 已取消。」is_error=false
    // TimedOut → 超时文案, is_error=false
    // DisabledSkipped → 未启用文案, is_error=false
}
```

- [ ] **Step 2-4:** 若核验通过则测试即文档；改写 Task 6/8 中失效的既有用例（`run_manual_reflection` 直调 → `ChatManualReflection` 或 engine 路径）。Commit：

```bash
git commit -am "test(runtime): #1796 反思终态三态文案判定覆盖"
```

---

### Task 12: L4 TUI 场景测试

**Files:**
- Test: `apps/cli/src/tui/app/scenario_tests/chat.rs`（对齐既有场景风格）

- [ ] **Step 1-4: 场景用例**

```rust
#[test]
fn live_reflection_activity_shows_spinner_then_terminal_notice() {
    // 1. 注入 Running root activity（purpose=Reflection）+ Running Reflection leaf
    //    → live status 显示 "Reflecting…"（经 ActivitySummaryAssembler/既有 widget 断言）
    // 2. 注入 terminal（Succeeded）→ spinner 消失
    // 3. CommandResultText 三态各一例：成功/失败(error 样式)/取消(非 error 样式)
}

#[test]
fn reflection_purpose_root_does_not_leak_into_conversation_queue() {
    // Manual Reflection Run 期间 is_processing=true（Run 事件驱动），
    // 终态后 is_processing=false（对齐 ui_event_tests.rs:506 的 queue 语义）
}
```

```bash
cargo test -p cli --lib tui
git commit -am "test(tui): #1796 L4 反思 spinner 出现/消失与三态 notice 场景"
```

---

### Task 13: 文档同步

**Files:**
- Modify: `docs/design/02-modules/memory/03-reflection.md`（§1 职责边界表补「状态发布」归属 Runtime loop 状态机；§4 补三触发的 Run 归属、`Reflecting` 态、Activity 可见性、取消语义口径（P1 现状 + P2 预告））
- Modify: `specs/3.3-tui-cli.md`（Reflection activity 展示规范：purpose=Reflection 的 root 参与 spinner、`"Reflecting…"` 文案、三态终态样式）

- [ ] **Step 1: 差异清单核对**——逐条对照 issue #1796「文档与代码双向校验门禁」，更新为 已对齐/已修正文档
- [ ] **Step 2: Commit**

```bash
git commit -am "docs(memory): #1796 反思状态发布与 Run 归属文档同步"
```

---

### Task 14: 全量验证与 PR

- [x] **Step 1: 全量测试与 clippy**

```bash
cargo test --workspace --all-targets
cargo test -p runtime --all-targets
cargo test -p sdk --all-targets
cargo test -p cli --all-targets
cargo clippy --workspace --all-targets
cargo fmt --all -- --check
```

证据：workspace 全量测试、runtime/sdk/cli 聚焦全量测试、clippy 与 fmt 均通过；完整架构守卫 `check-architecture-guards.sh --full` 通过（88 rules, 0 violations）。

- [x] **Step 2: 收尾退役检查**——旧的 Interval/PreCompact/Manual 执行点已移入 engine phase；`RunExecutionState` 已补 Run 级 Interval 一次性闸门。保留的 `run_manual_reflection` 为 Manual Reflection port 的正式方法，非已退役的旧裸编排路径；`reflection_tasks` 仍由生产端口与测试 fixture 使用。
- [x] **Step 3: 日志规范自查**——新增/修改的日志调用显式使用 `crate::LOG_TARGET`，未引入裸 target。
- [x] **Step 4: PR 前收尾**——已 `git pull origin main` 同步到最新主线；PR Test plan 将披露：①反思成功 Usage 计入及前后数字变化；②Manual Reflection 无 RunStep 时使用仅记账 UUIDv7；③缺陷 4 原有 Cancelled/TimedOut 文案已预先部分修复，本批保持并补测试；④文档核对与完整架构守卫命令。

### 根因修复补充记录

- [x] Interval 单 Run 去重：复现测试先行覆盖同一 Run 两个 `ModelStep::Complete`，再由 `RunExecutionState.interval_reflection_started` 在 phase 开始时一次性消耗闸门；miss 不消耗，失败/取消不回滚。
- [x] `ReflectionCompleted` 缺失返回状态：由 panic/`expect` 改为 `IllegalTransition` 结构化错误，并保留状态不变。
- [x] 反思文档与注释：同步修正 Run 归属、触发点、取消语义与 Manual Reflection 超时终态口径。

---

## Self-Review 记录

- **Spec 覆盖**：issue #1796 完成定义逐条 → Task 1（Reflecting 15 态+转移表）、Task 2（RunPurposeView::Reflection 经 Task 3/4）、Task 3/4/5（ActivityKindView+Detail+三触发发布+TUI）、Task 8（Manual Run 化四硬约束）、Task 6/7（Interval/PreCompact 插入）、Task 5（spinner 放宽+phase_label）、Task 11（三态文案）、Task 9（裁决 1）、Task 10（裁决 2）、Task 6（DisabledSkipped）、Task 12（L4）、Task 13（文档）、Task 14（日志+退役+验证）。L3 契约：无跨 BC PL 变更（issue 标注 N/A，sdk activity.rs 属观测 PL 扩展，由 Task 3 序列化测试覆盖）。
- **占位符扫描**：`advance_run_to_applying_response` 等测试辅助标注了「按既有模式对齐」——属测试基建，执行时以 tests.rs 既有辅助为准命名。
- **类型一致性**：`ReflectionPhasePort`/`ManualReflectionPort`/`ManualReflectionOutcome`/`RunPurpose` 在 Task 4/6/8 间签名已对齐；`record_successful_usage` 签名重构（Task 9）影响 `invocation.rs:321` 调用点与 `invocation_usage_tests.rs`，已声明。
