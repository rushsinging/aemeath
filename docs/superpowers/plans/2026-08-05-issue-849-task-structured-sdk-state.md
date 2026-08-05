# Task 结构化 SDK 状态变化实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 完成 #849 / #1058 留存链路：Task mutation 的提交事实不再被 Tool 丢弃，Runtime 不再按工具名或结果文本猜测变化，并通过 SDK 向 TUI 发布带 revision 的结构化 Task 状态；LLM 继续只看到现有 Tool Result。

**Architecture:** Task BC 继续以 `TaskCommandResult<T>` 作为原子事务结果，不引入 event sourcing、事件总线或 LLM 事件。Task Tool 将已提交 revision 和仅供 Hook 使用的少量事实映射成内部 `CommittedTaskChange`，通过现有 Tool execution outcome 传给 Runtime；Runtime 仅在收到该提交事实后查询一次 Task read model，构造结构化 `TaskStateView` 并发布 SDK `TaskStateChanged`。TUI 原子替换结构化状态并负责最终图标/文本渲染，不复制 Task 状态机，也不逐事件归约聚合。

**Tech Stack:** Rust 2021、Tokio、serde/schemars、Task `TaskAccess`、Tools Catalog/Execution、Runtime Tool round、SDK Published Language / JSON Schema、TUI TEA。

---

## 设计边界

### 必须保持

- LLM 仍只消费 `ToolResult.text` 和既有结构化 tool result；`CommittedTaskChange` **NEVER** 写入 LLM message、canonical session 或 tool result JSON。
- `TaskEvent` 仍是 Task BC 内部的提交事实；SDK 不逐项公开完整领域事件。
- SDK 发布的是带 revision 的完整**当前展示状态**，TUI 不实现 Task 状态机、不维护增量事件日志。
- 一轮内多个串行 Task mutation 只需在收尾时发布一次最新 `TaskStateChanged`；Hook facts 仍逐次处理且不得重复。
- 失败和幂等 no-op 没有 committed change，不触发 Task Hook，也不发布新的 SDK Task state。
- Session snapshot / restore 继续由 `TaskPersist` 和 Context 协调；本计划只要求恢复后发布的结构化状态与实时状态使用同一个 Runtime ACL 构造器，不改变落盘格式。

### 明确不做

- 不做 event sourcing、持久化 event log、ack/retry 或跨进程可靠事件投递。
- 不给 Batch 的每种内部转换新增 SDK 增量事件。
- 不让 TUI 直接依赖 `task` crate，也不让 SDK 依赖 Task backing。
- 不新增 Task 专属回调旁路；提交事实跟随现有 Tool execution outcome。
- 不把所有 Tool 扩展成通用业务 effect framework；仅新增职责明确的可选 `CommittedTaskChange`。

## 目标数据流

```text
TaskAccess mutation
  -> TaskCommandResult(value, events, committed revision)
  -> Task Tool
       LLM-visible: existing text + existing data
       Runtime-only: Option<CommittedTaskChange>
  -> ToolResult -> ToolSuccess -> Runtime ToolOutcome
  -> Runtime
       typed facts -> TaskCreated / TaskCompleted Hook
       committed change exists -> query TaskAccess once after round
  -> SDK ChatEvent::TaskStateChanged { session_id, state: TaskStateView }
  -> TUI-owned session-scoped TaskStateSnapshot
  -> ConversationModel accepts a new session or a non-decreasing revision
  -> ViewAssembler renders task lines
```

## 文件结构

### 新建

- `agent/features/tools/src/domain/task_change.rs`：定义 Runtime-only `CommittedTaskChange` / `TaskChangeFact`，不含展示文本或 Task backing handle。
- `agent/features/tools/src/domain/task_change_tests.rs`：锁定 committed/no-op 与 Hook fact 映射契约。
- `packages/sdk/src/task.rs`：定义 wire-safe `TaskStateView`、Batch/Task/status/priority DTO。
- `packages/sdk/src/task_tests.rs`：结构化状态 serde/schema 与字段完整性测试。
- `apps/cli/src/tui/model/conversation/task_status_tests.rs`：revision-aware 原子替换与结构化渲染测试。
- `.agents/hooks/check-task-state-pipeline.sh`：禁止工具名 mutation 清单、结果文本完成判断和 SDK 纯字符串 Task 状态复活。

### 修改

- `agent/features/tools/src/domain/tool.rs`：`TypedToolResult<T>` 与 `TypedToolAdapter` 透传可选 committed Task change。
- `agent/features/tools/src/domain/tool_types.rs`：legacy `ToolResult` 内部透传 committed Task change；保持 LLM/data 投影不变。
- `agent/features/tools/src/domain/published_language.rs`：`ToolSuccess` 增加 runtime-only committed Task change。
- `agent/features/tools/src/domain.rs`、`agent/features/tools/src/lib.rs`：只发布必要的窄类型。
- `agent/features/tools/src/adapters/execution.rs`：`ToolResult -> ToolSuccess` 无损映射 change。
- `agent/features/tools/src/adapters/task_create.rs`、`task_update.rs`、`task_block_by.rs`、`task_stop.rs`、`task_list_create.rs`、`task_list_complete.rs`：在丢弃 `TaskCommandResult` 前生成 committed change。
- 上述 adapter 的同级 `*_tests.rs`：覆盖 commit、no-op、失败和 Hook fact。
- `agent/features/runtime/src/application/tool/agent/runtime.rs`：`ToolExecutionOutcome -> ToolOutcome` 保留 change。
- `agent/features/runtime/src/application/tool/coordination.rs`：用 change 是否存在替代 `is_task_store_mutation(tool_name)`。
- `agent/features/runtime/src/application/loop_engine/chat/non_agent.rs`：用 typed facts 替代工具名和 `"Status: Completed"` 文本判断。
- `agent/features/runtime/src/application/loop_engine/chat/events.rs`：删除 mutation 工具名清单；将 `TasksSnapshot` 替换为 `TaskStateChanged`。
- `agent/features/runtime/src/application/loop_engine/chat/task_snapshot.rs`：收窄为结构化 Task state ACL 和 reminder 文本渲染的唯一选择逻辑。
- `agent/features/runtime/src/application/loop_engine/chat/task_snapshot_tests.rs`、`events_tests.rs`、`loop_runner_tests.rs`：覆盖 typed commit gating、结构化字段与单轮合并发布。
- `agent/features/runtime/src/application/loop_engine/chat/main_run_port.rs`：仅在 committed change 存在时发布最新结构化状态。
- `agent/features/runtime/src/adapters/sdk_event_mapper.rs` 及同级测试：Runtime state → SDK event 无损转换。
- `packages/sdk/src/chat_event.rs`、`lib.rs`、`wire.rs`：发布 `TaskStateChanged` 并纳入 schema components。
- `packages/sdk/schema/wire-components.schema.json`：由 xtask 重新生成，禁止手改。
- `apps/cli/src/tui/adapter/tui_runtime_event.rs`、`event_mapping.rs`、`event_mapping_tests.rs`、`agent_event.rs`：SDK DTO → TUI DTO → intent 逐层无损转换。
- `apps/cli/src/tui/model/conversation/task_status.rs`、`intent.rs`、`intent_impls.rs`、`runtime_state.rs`：保存结构化快照并按 revision 原子替换。
- `apps/cli/src/tui/view_assembler/live_status.rs` 及测试：从结构化状态生成现有 task lines。
- `apps/cli/src/tui/app/update/ui_event.rs`、`effect/session/processing/event_mapping.rs`：统一消费结构化 Task state。
- `docs/design/02-modules/task/01-domain-model.md`、`02-ports-and-published-language.md`：澄清内部 change 与 SDK 完整状态边界。
- `docs/design/03-engineering/04-testing-and-coverage.md`：更新 §11.10 的真实完成证据，不再声称由 #879 承接。
- `docs/design/03-engineering/01-architecture-guards.md`、`.agents/hooks/check-architecture-guards.sh`：登记新 Guard。

---

### Task 1: 定义最小 committed Task change

**Files:**
- Create: `agent/features/tools/src/domain/task_change.rs`
- Create: `agent/features/tools/src/domain/task_change_tests.rs`
- Modify: `agent/features/tools/src/domain.rs`
- Modify: `agent/features/tools/src/lib.rs`

- [ ] **Step 1: 写 committed/no-op/fact 失败测试**

在 `task_change_tests.rs` 使用真实 `TaskStore` 构造三类结果：创建 Task、`InProgress -> Completed`、重复设置同值 no-op。断言目标 API 具有以下行为：

```rust
let change = CommittedTaskChange::from_command_result(&created).unwrap();
assert_eq!(change.revision(), created.revision().unwrap().get());
assert!(matches!(change.facts(), [TaskChangeFact::Created { task_id }] if *task_id == created.value.id().get()));

let completed_change = CommittedTaskChange::from_command_result(&completed).unwrap();
assert!(matches!(completed_change.facts(), [TaskChangeFact::Completed { .. }]));

assert!(CommittedTaskChange::from_command_result(&no_op).is_none());
```

同时断言 subject/priority/dependency 等普通提交产生 `Some(change)` 但 `facts()` 为空：这些变化需要刷新 SDK state，不需要 Task Hook。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p tools task_change -- --nocapture`
Expected: FAIL，`CommittedTaskChange` / `TaskChangeFact` 尚不存在。

- [ ] **Step 3: 实现最小内部类型**

实现职责化类型，不持有 Task store、快照或展示字符串：

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedTaskChange {
    revision: u64,
    facts: Vec<TaskChangeFact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskChangeFact {
    Created { task_id: u64 },
    Completed { task_id: u64 },
}
```

`from_command_result<T>(&task::TaskCommandResult<T>) -> Option<Self>` 必须：

- 仅在 `revision() == Some(_)` 时返回 `Some`；
- 从 `TaskEvent::TaskCreated` 映射 Created；
- 仅从 `TaskEvent::TaskStatusChanged { to: Completed, .. }` 映射 Completed；
- 忽略其他事件但保留 committed revision；
- 保持 facts 的 Task 事务事件顺序。

- [ ] **Step 4: 运行定向测试**

Run: `cargo test -p tools task_change -- --nocapture`
Expected: PASS。

- [ ] **Step 5: 提交本层变更**

```bash
git add agent/features/tools/src/domain/task_change.rs \
  agent/features/tools/src/domain/task_change_tests.rs \
  agent/features/tools/src/domain.rs agent/features/tools/src/lib.rs
git commit -m "feat(task): define committed task change"
```

### Task 2: 通过现有 Tool pipeline 无损传递 change

**Files:**
- Modify: `agent/features/tools/src/domain/tool.rs`
- Modify: `agent/features/tools/src/domain/tool_types.rs`
- Modify: `agent/features/tools/src/domain/published_language.rs`
- Modify: `agent/features/tools/src/adapters/execution.rs`
- Test: Create `agent/features/tools/src/domain/tool_tests.rs` 并在 `agent/features/tools/src/domain.rs` 以 `#[path = "domain/tool_tests.rs"]` 注册
- Test: `agent/features/tools/src/adapters/catalog_execution_contract_tests.rs`

- [ ] **Step 1: 写 Tool pipeline 失败契约测试**

构造一个仅测试用 typed tool，返回普通 text/data 和 `CommittedTaskChange`。分别断言：

- `TypedToolResult -> ToolResult` 保留 change；
- `map_legacy_result -> ToolSuccess` 保留 change；
- text/data 与现有值完全相同；
- failure/cancel/timeout/suspension 不携带 committed Task change。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p tools committed_task_change_survives_tool_execution_pipeline -- --exact`
Expected: FAIL，现有 Tool 结果类型没有 change 字段。

- [ ] **Step 3: 为现有结果类型增加窄字段**

在三个内部阶段增加同名可选字段：

```rust
pub task_change: Option<CommittedTaskChange>
```

为 `TypedToolResult<T>` 提供：

```rust
#[must_use]
pub fn with_task_change(mut self, change: Option<CommittedTaskChange>) -> Self
```

所有通用 success/error 构造器默认 `None`；`TypedToolAdapter`、`ToolResult::from_*`、`map_legacy_result` 必须逐层原样传递。该字段不得进入 `ToolSuccess.data`、content 或 serde wire 输出；若 `ToolSuccess` 当前必须 derive serde，则用 `#[serde(skip)]` 明确其 Runtime-only 属性，并为 deserialize 默认 `None`。

- [ ] **Step 4: 运行 Tools 契约测试**

Run: `cargo test -p tools committed_task_change_survives_tool_execution_pipeline -- --exact`
Run: `cargo test -p tools catalog_execution_contract_tests -- --nocapture`
Expected: PASS，且既有 tool text/data 契约不变。

- [ ] **Step 5: 提交 Tool pipeline 变更**

```bash
git add agent/features/tools/src/domain/tool.rs \
  agent/features/tools/src/domain/tool_types.rs \
  agent/features/tools/src/domain/published_language.rs \
  agent/features/tools/src/adapters/execution.rs \
  agent/features/tools/src/domain/*tests.rs \
  agent/features/tools/src/adapters/catalog_execution_contract_tests.rs
git commit -m "refactor(tools): carry committed task changes"
```

### Task 3: Task Tools 保留真实提交事实

**Files:**
- Modify: `agent/features/tools/src/adapters/task_create.rs`
- Modify: `agent/features/tools/src/adapters/task_update.rs`
- Modify: `agent/features/tools/src/adapters/task_block_by.rs`
- Modify: `agent/features/tools/src/adapters/task_stop.rs`
- Modify: `agent/features/tools/src/adapters/task_list_create.rs`
- Modify: `agent/features/tools/src/adapters/task_list_complete.rs`
- Test: 对应同级 `task_*_tests.rs`；内嵌测试先按仓库规范迁到同级文件后修改

- [ ] **Step 1: 为六个 mutation Tool 写失败测试**

每个 adapter 至少覆盖：

- 成功 commit：`result.task_change == Some` 且 revision 与 `TaskAccess::revision()` 一致；
- 失败：change 为 `None`；
- 可构造的 no-op：change 为 `None`；
- TaskCreate 产生 Created fact；
- TaskUpdate 只有真实进入 Completed 才产生 Completed fact；subject/priority/dependency change 只有 revision、无 Hook fact；
- TaskListCreate/Complete 即使 Task BC 当前没有 Batch `TaskEvent`，仍因 committed revision 产生 change。

- [ ] **Step 2: 运行 Task Tool 测试确认失败**

Run: `cargo test -p tools task_create -- --nocapture`
Run: `cargo test -p tools task_update -- --nocapture`
Run: `cargo test -p tools task_block_by -- --nocapture`
Run: `cargo test -p tools task_stop -- --nocapture`
Run: `cargo test -p tools task_list_create -- --nocapture`
Run: `cargo test -p tools task_list_complete -- --nocapture`
Expected: FAIL，adapter 仍只取 `result.value`。

- [ ] **Step 3: 在每个 adapter 中先映射 change 再消费 value**

统一采用同一模式，禁止复制 event 匹配逻辑：

```rust
let command_result = match self.access.create_task(spec, timestamp) {
    Ok(result) => result,
    Err(error) => return TypedToolResult::error(error.to_string()),
};
let task_change = CommittedTaskChange::from_command_result(&command_result);
let created = command_result.value;
TypedToolResult::success(text, data).with_task_change(task_change)
```

其他 Task Tool 使用相同 helper；不得按工具名、目标 status 或输出文本手工伪造 change。

- [ ] **Step 4: 运行全部 Task Tool 测试**

Run: `cargo test -p tools task_ -- --nocapture`
Expected: PASS。

- [ ] **Step 5: 提交 Task Tool 变更**

```bash
git add agent/features/tools/src/adapters/task_*.rs
git commit -m "fix(task): preserve tool mutation commit facts"
```

### Task 4: Runtime 用 typed change 驱动刷新与 Hook

**Files:**
- Modify: `agent/features/runtime/src/application/tool/agent/runtime.rs`
- Modify: `agent/features/runtime/src/application/tool/coordination.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/non_agent.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/events.rs`
- Delete or rewrite: `agent/features/runtime/src/application/loop_engine/chat/events_tests.rs`
- Test: `agent/features/runtime/src/application/loop_engine/chat/loop_runner_tests.rs`
- Create: `agent/features/runtime/src/application/loop_engine/chat/non_agent_tests.rs`，并在 `non_agent.rs` 以外置测试模块注册

- [ ] **Step 1: 写 Runtime change gating 失败测试**

覆盖四种 ToolExecution：

1. 名称为 `TaskUpdate` 但无 committed change：不刷新、不触发 Hook；
2. 任意名称但携带 committed change：刷新；
3. Completed fact：只触发一次 TaskCompleted Hook；
4. Tool text 含 `Status: Completed` 但无 fact：不触发 Hook。

再覆盖同一轮两个 committed changes：Observer 只收到一次“需要发布最新 Task state”，但两个 Hook facts 都按执行顺序处理。

- [ ] **Step 2: 运行 Runtime 测试确认失败**

Run: `cargo test -p runtime committed_task_change_drives_state_refresh -- --exact`
Run: `cargo test -p runtime task_hooks_use_typed_facts_not_tool_text -- --exact`
Expected: FAIL，当前仍依赖工具名与结果文本。

- [ ] **Step 3: 让 Runtime legacy outcome 保留 change**

在 Runtime 私有 `ToolOutcome` 增加 `task_change: Option<CommittedTaskChange>`，并在 `legacy_outcome(ToolExecutionOutcome)` 的 Success 分支复制；其他终态为 `None`。不得把 change 放入 materialized tool result message。

- [ ] **Step 4: 删除工具名 mutation 清单**

删除：

```rust
is_task_store_mutation(tool_name)
```

在 `finalize_tool_round_results` 中改为：

```rust
let has_committed_task_change = results
    .iter()
    .any(|execution| execution.outcome.task_change.is_some());
```

同步把 `ToolRoundObserver::results_materialized` 参数重命名为 `has_committed_task_change`。

- [ ] **Step 5: 用 typed facts 驱动 Hook**

`run_task_hooks` 不再检查 `call.name` 或输出内容，而是遍历该次 execution 的 `task_change.facts()`：

- Created → `HookInvocation::TaskCreated`；
- Completed → `HookInvocation::TaskCompleted`；
- 无 facts → 不触发。

为保持 Hook 兼容，`TaskInput.tool_input/tool_output` 继续使用当前调用数据，但**触发条件**只能来自 typed fact。

- [ ] **Step 6: 运行 Runtime 定向测试**

Run: `cargo test -p runtime committed_task_change_drives_state_refresh -- --exact`
Run: `cargo test -p runtime task_hooks_use_typed_facts_not_tool_text -- --exact`
Run: `cargo test -p runtime loop_engine::chat --lib`
Expected: PASS。

- [ ] **Step 7: 提交 Runtime typed consumption**

```bash
git add agent/features/runtime/src/application/tool/agent/runtime.rs \
  agent/features/runtime/src/application/tool/coordination.rs \
  agent/features/runtime/src/application/loop_engine/chat/non_agent.rs \
  agent/features/runtime/src/application/loop_engine/chat/events.rs \
  agent/features/runtime/src/application/loop_engine/chat/events_tests.rs \
  agent/features/runtime/src/application/loop_engine/chat/loop_runner_tests.rs
git commit -m "refactor(runtime): consume committed task changes"
```

### Task 5: 定义 SDK 结构化 Task state Published Language

**Files:**
- Create: `packages/sdk/src/task.rs`
- Create: `packages/sdk/src/task_tests.rs`
- Modify: `packages/sdk/src/lib.rs`
- Modify: `packages/sdk/src/chat_event.rs`
- Modify: `packages/sdk/src/wire.rs`
- Generated: `packages/sdk/schema/wire-components.schema.json`

- [ ] **Step 1: 写 SDK DTO 与 schema 失败测试**

测试固定构造以下状态并 serde round-trip：

```rust
TaskStateView {
    session_id: "session-a".into(),
    revision: 42,
    current_batch: Some(TaskBatchView {
        id: 7,
        summary: Some("ship".into()),
        status: TaskBatchStatusView::Active,
    }),
    total: 3,
    completed: 1,
    in_progress: 1,
    items: vec![/* completed, in-progress, blocked pending */],
    hidden_count: 0,
}
```

断言每个 item 保留：`id`、Batch 内 `sequence`、`subject`、typed `status`、typed `priority`、`blocked_by_sequences`；`session_id` 用于 Session 切换时建立独立 revision epoch；空当前 Batch 使用 `None + empty items`，revision 仍保留。断言 components document 含所有 Task DTO，且没有 `lines: Vec<String>` 作为状态真相。

- [ ] **Step 2: 运行 SDK 测试确认失败**

Run: `cargo test -p sdk task_state_view_round_trips_without_field_loss -- --exact`
Run: `cargo test -p sdk wire_components_include_task_state -- --exact`
Expected: FAIL，结构化类型尚不存在。

- [ ] **Step 3: 实现 wire-safe DTO**

定义并 derive `Serialize`、`Deserialize`、`JsonSchema`、`Clone`、`Debug`、`PartialEq`、`Eq`：

```rust
pub struct TaskStateView {
    pub session_id: String,
    pub revision: u64,
    pub current_batch: Option<TaskBatchView>,
    pub total: usize,
    pub completed: usize,
    pub in_progress: usize,
    pub items: Vec<TaskItemView>,
    pub hidden_count: usize,
}

pub struct TaskBatchView {
    pub id: u64,
    pub summary: Option<String>,
    pub status: TaskBatchStatusView,
}

pub struct TaskItemView {
    pub id: u64,
    pub sequence: u64,
    pub subject: String,
    pub status: TaskItemStatusView,
    pub priority: TaskPriorityView,
    pub blocked_by_sequences: Vec<u64>,
}
```

状态与优先级使用封闭 enum + `#[serde(rename_all = "snake_case")]`。将 `ChatEvent::TasksSnapshot` 替换为：

```rust
TaskStateChanged { state: Box<TaskStateView> }
```

若历史 session 需要读取旧 variant，仅在 decode adapter 保留兼容读取，生产路径和新 schema 不继续发布旧事件。

- [ ] **Step 4: 注册并生成 schema**

在 `wire::components_document` 注册所有 Task DTO，然后运行：

Run: `cargo run -p xtask -- sdk-wire-schema write`
Expected: `packages/sdk/schema/wire-components.schema.json` 确定性更新。

- [ ] **Step 5: 运行 SDK 验证**

Run: `cargo test -p sdk task_ -- --nocapture`
Run: `cargo run -p xtask -- sdk-wire-schema check`
Expected: PASS。

- [ ] **Step 6: 提交 SDK Published Language**

```bash
git add packages/sdk/src/task.rs packages/sdk/src/task_tests.rs \
  packages/sdk/src/lib.rs packages/sdk/src/chat_event.rs packages/sdk/src/wire.rs \
  packages/sdk/schema/wire-components.schema.json
git commit -m "feat(sdk): publish structured task state"
```

### Task 6: Runtime 构造唯一结构化 Task state ACL

**Files:**
- Modify: `agent/features/runtime/src/application/loop_engine/chat/task_snapshot.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/task_snapshot_tests.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/events.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/main_run_port.rs`
- Modify: `agent/features/runtime/src/adapters/sdk_event_mapper.rs`
- Test: `agent/features/runtime/src/adapters/sdk_event_mapper_tests.rs`
- Test: `agent/features/runtime/src/application/loop_engine/chat/loop_runner_tests.rs`

- [ ] **Step 1: 写结构化 ACL 失败测试**

使用真实 TaskStore 创建同一 Batch 的 completed/in-progress/blocked pending Task，断言 `build_task_state_view`：

- revision 等于 `TaskAccess::revision()`；
- current batch 字段完整；
- items 顺序沿用当前 completed → in-progress → pending 的窗口规则；
- blocked-by 从内部 TaskId 转成 Batch 内 sequence；
- `max_lines` 截断产生准确 `hidden_count`；
- 无 active batch 时保留 revision，并发布 `current_batch: None/items: []`，用于 TUI 清空旧列表。

- [ ] **Step 2: 运行 Runtime ACL 测试确认失败**

Run: `cargo test -p runtime task_state_view_preserves_structured_fields -- --exact`
Expected: FAIL，当前只生成 `TaskStatusView.lines`。

- [ ] **Step 3: 将快照构造器改成结构化 ACL**

把 `build_task_snapshot` 重命名为 `build_task_state_view`，返回 SDK DTO。保留唯一的 Task 窗口选择算法；`build_task_reminder` 先调用同一结构化构造/选择 helper，再只为 LLM reminder 渲染文本，避免复制排序和截断规则。

- [ ] **Step 4: 替换 Runtime/SDK 事件**

- `RuntimeStreamEvent::TasksSnapshot` → `TaskStateChanged { state }`；
- `ChatToolRoundObserver::results_materialized` 仅在 `has_committed_task_change` 时查询一次 TaskAccess；
- mapper 1:1 转成 `sdk::ChatEvent::TaskStateChanged`，并把当前 `session_id` 绑定到 state；
- 即使 Batch 自动关闭导致无 active batch，也必须发布空 state 以清空 TUI。

- [ ] **Step 5: 写一轮多 mutation 场景测试**

在 `loop_runner_tests.rs` 执行一轮两个串行 Task mutation，断言：

- 两个 ToolResult 照常进入 LLM message；
- SDK 只收到一个 TaskStateChanged；
- revision 是第二个 mutation 后的最新值；
- state 同时包含两次修改结果；
- 不再依赖 tool name allowlist。

- [ ] **Step 6: 运行 Runtime 与 mapper 测试**

Run: `cargo test -p runtime task_state -- --nocapture`
Run: `cargo test -p runtime sdk_event_mapper -- --nocapture`
Run: `cargo test -p runtime one_round_publishes_latest_task_state_once -- --exact`
Expected: PASS。

- [ ] **Step 7: 提交 Runtime ACL**

```bash
git add agent/features/runtime/src/application/loop_engine/chat/task_snapshot.rs \
  agent/features/runtime/src/application/loop_engine/chat/task_snapshot_tests.rs \
  agent/features/runtime/src/application/loop_engine/chat/events.rs \
  agent/features/runtime/src/application/loop_engine/chat/main_run_port.rs \
  agent/features/runtime/src/adapters/sdk_event_mapper.rs \
  agent/features/runtime/src/adapters/sdk_event_mapper_tests.rs \
  agent/features/runtime/src/application/loop_engine/chat/loop_runner_tests.rs
git commit -m "feat(runtime): publish structured task state"
```

### Task 7: TUI 原子消费结构化状态并负责渲染

**Files:**
- Modify: `apps/cli/src/tui/adapter/tui_runtime_event.rs`
- Modify: `apps/cli/src/tui/adapter/event_mapping.rs`
- Modify: `apps/cli/src/tui/adapter/event_mapping_tests.rs`
- Modify: `apps/cli/src/tui/adapter/agent_event.rs`
- Modify: `apps/cli/src/tui/model/conversation/task_status.rs`
- Create: `apps/cli/src/tui/model/conversation/task_status_tests.rs`
- Modify: `apps/cli/src/tui/model/conversation/intent.rs`
- Modify: `apps/cli/src/tui/model/conversation/intent_impls.rs`
- Modify: `apps/cli/src/tui/model/conversation/runtime_state.rs`
- Modify: `apps/cli/src/tui/view_assembler/live_status.rs`
- Modify: `apps/cli/src/tui/app/update/ui_event.rs`
- Modify: `apps/cli/src/tui/effect/session/processing/event_mapping.rs`

- [ ] **Step 1: 写 SDK → TUI adapter 字段完整性失败测试**

固定构造 `sdk::ChatEvent::TaskStateChanged`，断言 revision、Batch、Task、priority、blocked sequences、hidden count 无损到达 TUI-owned DTO。测试不得只比较最终 lines。

- [ ] **Step 2: 写 model revision 失败测试**

覆盖：

- same session 的 revision 41 → 42：原子替换；
- same session 的重复 revision 42：幂等；
- same session 的旧 revision 41：不得覆盖 42；
- 切换到新的 `session_id` 且 revision 较小：建立新的 revision epoch 并替换旧 Session 状态；
- 新 Session 的 revision 1 + `current_batch: None`：清空 items 和 lines；
- model 不执行 Pending/InProgress/Completed 合法性判断，只消费 Runtime 权威状态。

- [ ] **Step 3: 运行 TUI 测试确认失败**

Run: `cargo test -p cli task_state_event_preserves_all_fields -- --exact`
Run: `cargo test -p cli task_state_rejects_older_revision -- --exact`
Expected: FAIL，当前模型只保存 lines。

- [ ] **Step 4: 实现 TUI-owned 结构化 snapshot 与单一 intent**

`TaskStatusSnapshot` 改为 TUI-owned 纯值结构；新增 `ReplaceTaskState` intent，同时替代 `UpdateTaskStatus` 与 `UpdateTaskLines` 的 Task 事件路径。adapter 负责 SDK DTO → TUI DTO；model 只做 revision 比较和原子替换。

- [ ] **Step 5: 将最终文本渲染移到 ViewAssembler**

`live_status.rs` 从结构化 items 生成现有显示：

- Completed → `✓`；
- InProgress → `■`；
- Pending → `□`；
- `#<sequence> <subject>`；
- 依赖显示 `(blocked by #N, #M)`；
- 首行 `━━ Tasks: completed/total ━━`；
- `hidden_count > 0` 显示 `… +N more`。

颜色/selection 层继续消费最终 `task_lines`，不需要改变视觉协议。

- [ ] **Step 6: 补最终场景测试**

在现有 TUI scenario/harness 中依次投递 revision 1～5 的结构化 state，覆盖创建列表、创建依赖任务、开始、完成前置、全部完成后空 state。逐步断言 task window 内容、blocked sequence、最终清空，并断言旧 revision 回放不会恢复旧列表。

- [ ] **Step 7: 运行 TUI 定向测试**

Run: `cargo test -p cli task_state -- --nocapture`
Run: `cargo test -p cli live_status -- --nocapture`
Run: `cargo test -p cli task_state_scenario -- --exact`
Expected: PASS。

- [ ] **Step 8: 提交 TUI 消费链**

```bash
git add apps/cli/src/tui/adapter apps/cli/src/tui/model/conversation \
  apps/cli/src/tui/view_assembler/live_status.rs \
  apps/cli/src/tui/app/update/ui_event.rs \
  apps/cli/src/tui/effect/session/processing/event_mapping.rs
git commit -m "feat(tui): render structured task state"
```

### Task 8: 实时与 Resume 状态语义对齐

**Files:**
- Modify: `agent/features/runtime/src/application/loop_engine/chat/session_driver/run_launch.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/session_driver_support_tests.rs`
- Modify: `agent/features/context/tests/main_session_wiring.rs`（仅补恢复后的权威 TaskAccess 证据，若现有断言不足）
- Test: `apps/cli/src/tui/app/scenario_tests/interaction.rs`

- [ ] **Step 1: 写实时/恢复等价失败场景**

建立一个含 active Batch、Completed/InProgress/blocked Pending Task 的 TaskSnapshot：

1. 实时 mutation 后捕获 `TaskStateChanged`；
2. 保存 Session；
3. 用新 TaskStore resume；
4. 通过同一个 `build_task_state_view` 构造恢复状态；
5. 断言除事件时机外，revision、Batch、items、stats、blocked sequence、hidden count 完全相等。

另加 archived/no-current-batch 场景，证明恢复后空 state 会清除 TUI 旧列表。

- [ ] **Step 2: 运行等价测试确认失败**

Run: `cargo test -p runtime realtime_and_resumed_task_state_are_equivalent -- --exact`
Run: `cargo test -p cli resumed_empty_task_state_clears_live_status -- --exact`
Expected: 至少一个 FAIL，当前 Resume/TUI 没有统一结构化状态入口。

- [ ] **Step 3: 复用唯一 Runtime ACL 入口**

不得在 Context、SDK 或 TUI 重建 Task view。恢复完成后由 Runtime 读取已安装的 `TaskAccess`，调用同一 `build_task_state_view` 并发布 `TaskStateChanged`；Context 只负责 restore。

- [ ] **Step 4: 运行恢复相关测试**

Run: `cargo test -p context main_session_wiring -- --nocapture`
Run: `cargo test -p runtime realtime_and_resumed_task_state_are_equivalent -- --exact`
Run: `cargo test -p cli resumed_empty_task_state_clears_live_status -- --exact`
Expected: PASS。

- [ ] **Step 5: 提交恢复链测试与接线**

```bash
git add agent/features/context/tests/main_session_wiring.rs \
  agent/features/runtime/src/application/loop_engine/chat/session_driver/run_launch.rs \
  agent/features/runtime/src/application/loop_engine/chat/session_driver_support_tests.rs \
  apps/cli/src/tui/app/scenario_tests/interaction.rs
git commit -m "test(task): align live and resumed task state"
```

### Task 9: 增加防回归 Guard 并清理旧路径

**Files:**
- Create: `.agents/hooks/check-task-state-pipeline.sh`
- Modify: `.agents/hooks/check-architecture-guards.sh`
- Modify: `docs/design/03-engineering/01-architecture-guards.md`
- Remove/modify: 所有 `TasksSnapshot`、`TaskStatusView { lines }`、`UpdateTaskLines` 生产引用

- [ ] **Step 1: 编写 Guard 规则**

Guard 至少阻止生产代码出现：

- `is_task_store_mutation`；
- Runtime 对 `"Status: Completed"` 的 Task Hook判断；
- `ChatEvent::TasksSnapshot` / `RuntimeStreamEvent::TasksSnapshot`；
- SDK `TaskStatusView` 仅含 `lines`；
- Task mutation adapter 直接 `Ok(result) => result.value` 而未调用 `CommittedTaskChange::from_command_result`。

Guard 应用结构化路径/符号检查，测试目录可排除但不得使用宽泛整目录白名单。

- [ ] **Step 2: 运行单 Guard 与总编排**

Run: `bash .agents/hooks/check-task-state-pipeline.sh`
Run: `bash .agents/hooks/check-architecture-guards.sh --fast`
Expected: PASS。

- [ ] **Step 3: 制造故意违规**

临时在 Runtime 生产文件加入旧工具名函数或文本判断，确认：

Run: `bash .agents/hooks/check-task-state-pipeline.sh`
Expected: exit 2；恢复临时改动后再次 PASS。

- [ ] **Step 4: 清理旧兼容路径**

Run: `rg 'TasksSnapshot|TaskStatusView|is_task_store_mutation|Status: Completed|UpdateTaskLines' agent/features/runtime packages/sdk apps/cli/src/tui`
Expected: 只剩明确的历史 decode compatibility 或 Guard/测试禁止字符串；所有生产发布与消费路径使用 `TaskStateChanged`。

- [ ] **Step 5: 提交 Guard 与清理**

```bash
git add .agents/hooks/check-task-state-pipeline.sh \
  .agents/hooks/check-architecture-guards.sh \
  docs/design/03-engineering/01-architecture-guards.md \
  agent/features/runtime packages/sdk apps/cli/src/tui
git commit -m "chore(task): guard structured state pipeline"
```

### Task 10: 回写设计、测试矩阵与 Issue 证据

**Files:**
- Modify: `docs/design/02-modules/task/01-domain-model.md`
- Modify: `docs/design/02-modules/task/02-ports-and-published-language.md`
- Modify: `docs/design/03-engineering/04-testing-and-coverage.md`

- [ ] **Step 1: 更新 Task 设计边界**

明确记录：

- `TaskCommandResult.events/revision` 在 Tool adapter 转成 Runtime-only committed change；
- LLM 不感知 change；
- SDK 发布完整结构化当前状态而非原始 TaskEvent；
- TUI 按 revision 原子替换，不复制 Task 状态机；
- Batch commit 即使没有 Hook fact，仍通过 revision 触发状态刷新。

- [ ] **Step 2: 更新 #1058 L0–L5 矩阵**

将 §11.10 的 #879 “实现缺口”条目替换为可追溯证据路径：

- L1/L2：CommittedTaskChange 与 Tool adapter；
- L2：Runtime commit gating 和 Hook facts；
- L3：SDK DTO/schema 与 mapper；
- L3/L4：TUI adapter/model/view；
- L4：完整旅程与实时/Resume 等价；
- L0：新 Guard、schema freshness、production reachability。

- [ ] **Step 3: 执行完整验证**

Run: `cargo fmt --all -- --check`
Run: `cargo test -p task -p tools -p runtime -p sdk -p context -p cli`
Run: `cargo test --workspace`
Run: `cargo run -p xtask -- sdk-wire-schema check`
Run: `cargo run -p xtask -- production-reachability .`
Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
Run: `.agents/hooks/check-architecture-guards.sh --full`
Run: `git diff --check`
Expected: 全部 PASS；首次失败必须记录和修复，不得用重跑成功覆盖。

- [ ] **Step 4: 检查死代码和旧公开面**

Run: `rg 'TasksSnapshot|TaskStatusView|UpdateTaskLines|set_task_lines|is_task_store_mutation' agent packages apps`
Run: `cargo check --workspace`
Expected: 无生产旧路径；若存在历史兼容 reader，文档记录退出条件和 owner。

- [ ] **Step 5: 提交文档与最终验证记录**

```bash
git add docs/design/02-modules/task \
  docs/design/03-engineering/04-testing-and-coverage.md
git commit -m "docs(task): record structured sdk state evidence"
```

- [ ] **Step 6: PR 前同步主线**

Run: `git pull origin main`
Expected: 无冲突；若有冲突，逐项保留双方测试与约束并重跑 Task 10 Step 3。

- [ ] **Step 7: 更新 GitHub 追踪（实施完成后执行）**

使用 `gh issue comment/edit --repo rushsinging/aemeath`：

- 在真正承接本计划的 leaf Issue 回写验证证据；
- 更新 #1058，移除“由 #879 承接”的过期结论；
- 更新 #849 测试审查状态；
- 不自行关闭 #1058 或 #849，等待用户确认。

---

## 验收标准

- Task Tool 不再静默丢弃真实 commit revision；失败/no-op 不产生 committed change。
- Runtime 生产代码不含 Task mutation 工具名清单，也不解析 Tool Result 文本判断完成。
- LLM message、tool result JSON、Session canonical messages 中不存在 `CommittedTaskChange`。
- `TaskStateChanged` 携带 `session_id`、revision、当前 Batch、结构化 Task items、依赖 sequence、统计和 hidden count。
- TUI 只按同一 `session_id` 内的非递减 revision 原子替换结构化状态；收到新 Session 的 state 时建立新的 revision epoch，最终视觉与现有 Task window 兼容。
- 一轮多个 Task mutation 只发布一个最新完整 state；Created/Completed Hook facts 不丢失、不重复。
- 实时 mutation 与 Session Resume 对同一 TaskSnapshot 产生等价 SDK/TUI 状态。
- schema freshness、逐层测试、L4 场景、workspace test/clippy、production reachability 和 architecture guards 全部通过。

## 风险与控制

- **通用 Tool result 改动面较大：** 只加可选 Runtime-only 字段，所有非 Task Tool 默认 `None`，先用 Tools L3 contract 锁定 text/data 不变。
- **序列化泄漏到 LLM/Session：** `ToolSuccess` 字段显式 skip serde，并测试 materialized message 不含 revision/facts。
- **一轮多 mutation 时发布旧状态：** Runtime 在 tool round 完成后从 TaskAccess 查询一次，不在每个 receipt 内携带快照。
- **TUI 旧事件乱序覆盖：** model 保存 revision，拒绝较旧 state；相同 revision 幂等。
- **恢复后旧列表残留：** 无 current batch 仍发布带最新 revision 的空 state。
- **视觉行为漂移：** 迁移前先锁定现有 header、排序、图标、blocked-by、hidden count 快照；仅移动渲染所有权，不重新设计 UI。
