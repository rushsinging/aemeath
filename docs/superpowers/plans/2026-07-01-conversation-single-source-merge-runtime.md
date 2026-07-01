# Plan: ConversationModel 单一真相源 — RuntimeModel 合并 + Intent trait 分发

**Spec**: `docs/superpowers/specs/2026-07-01-conversation-single-source-merge-runtime.md`
**日期**: 2026-07-01

## 执行策略

大型机械重构。按"自底向上"顺序：先搬迁类型、再改 intent 体系、再改 adapter/reducer、再改渲染层、最后清理。

每步完成后 `cargo check -p cli` 必须通过；关键节点跑 `cargo test -p cli` + `cargo clippy -p cli`。

---

## Phase 1: 类型搬迁（RuntimeModel 字段 → ConversationModel）

### Step 1.1: 运行态类型搬迁到 conversation 模块

把 `model/runtime/` 下的纯数据类型搬到 `model/conversation/` 下（或 `model/common/`），保持 `model/runtime/` 原文件暂不删除（re-export 过渡）。

**搬迁的类型**：
- `SpinnerModel` / `SpinnerPhase` / `HookOutcome`（`runtime/spinner.rs`）
- `WorkspaceState` / `WorktreeKind`（`runtime/workspace.rs`）
- `UsageSummary`（`runtime/usage.rs`）
- `TaskStatusSnapshot`（`runtime/task_status.rs`）
- `ProcessingJob` / `ProcessingStatus`（`runtime/processing_job.rs`）
- `StatusNotice` / `StatusNoticeKind`（`runtime/status_notice.rs`）
- `CompactProgressModel`（`runtime/compact_progress.rs`）

**验证**: `cargo check -p cli`

### Step 1.2: ConversationModel 增加运行态字段

在 `ConversationModel` struct 中加入所有从 RuntimeModel 搬来的字段。`Default` 实现对应初始化。暂不接线。

**验证**: `cargo check -p cli`

---

## Phase 2: Intent trait 分发体系

### Step 2.1: 定义 ConversationUpdate trait

新增 `model/conversation/update.rs`：

```rust
pub trait ConversationUpdate {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange>;
}
```

### Step 2.2: 现有 ConversationIntent variant 拆 struct

把 `ConversationIntent` enum 的每个 variant 拆成独立 struct。每个 struct `impl ConversationUpdate`，逻辑从 `ConversationModel` 的私有方法搬入。

### Step 2.3: RuntimeIntent variant 拆 struct

把 `RuntimeIntent` enum 的每个 variant 也拆成独立 struct，`impl ConversationUpdate`，逻辑从 `RuntimeModel::apply` 搬入。这些 struct 也放进 conversation 模块。

### Step 2.4: ConversationIntent enum 改为传输容器

新 enum 只包装 struct 变体，`impl ConversationUpdate` 做 match 转发。`apply` 改为泛型分发。

### Step 2.5: RuntimeChange 合并到 ConversationChange

把 `RuntimeChange` variant 合入 `ConversationChange`。`root_reducer` 中删除 `runtime` 分支逻辑。

**验证**: `cargo check -p cli` + `cargo test -p cli`（intent 单元测试适配新结构）

---

## Phase 3: 删除 RuntimeModel 中间层

### Step 3.1: 删除 RuntimeObservation

把 `agent_event.rs` 中 `runtime_observation_from_ui_event` + `ToolFlowProjector` 合并为直接 UiEvent → ConversationIntent 映射。删除 `RuntimeObservation` enum 和 `model/runtime_observation.rs`。

### Step 3.2: 删除 RuntimeModel

删除 `model/runtime/model.rs`、`model/runtime/intent.rs`、`model/runtime/change.rs`。所有引用改为 `ConversationModel`。

### Step 3.3: TuiModel 删除 runtime 字段

`TuiModel` struct 删除 `runtime: RuntimeModel`。所有 `model.runtime.xxx` 改为 `model.conversation.xxx`。

### Step 3.4: AgentEventMapping 删除 runtime 字段

`AgentEventMapping` 删除 `runtime: Vec<RuntimeIntent>`。原 runtime intent 全部走 `conversation` 字段。`root_reducer::reduce_agent_event` 删除 runtime 循环。

**验证**: `cargo check -p cli` + `cargo test -p cli`

---

## Phase 4: Spinner 写入收敛

### Step 4.1: map_agent_event 集中产出 spinner intent

在 `map_agent_event` 中，为每个 UiEvent 产出对应的 `SetSpinnerPhase` / `StopSpinner` intent（见 spec 表格）。

ToolResult 的 remaining：在 `map_agent_event` 产出 intent 批次时，ObserveToolResult 排在 SetSpinnerPhase 之前，apply 顺序执行后 model 已更新，SetSpinnerPhase 的 update 方法从 model 数 running tool_calls。

### Step 4.2: 删除 update_ui 命令式 spinner 调用

删除 `app/update/spinner.rs`（`spinner_phase()` / `spinner_stop()` 方法）。删除 `update_ui` 中所有 `self.spinner_phase()` / `self.spinner_stop()` 调用（约 20 处）。

### Step 4.3: 删除 SetCompactProgress 自动 spinner

当前 `RuntimeModel::SetCompactProgress` 中自动启动 spinner 的逻辑搬到 `map_agent_event`：收到 CompactProgress 时产出 `SetSpinnerPhase(Compacting)` + `SetCompactProgress`。

**验证**: `cargo check -p cli` + `cargo test -p cli`

---

## Phase 5: 渲染层适配

### Step 5.1: LiveStatusAssembler 改读 ConversationModel

`LiveStatusAssembler::assemble` 入参从 `runtime: &RuntimeModel` 改为从 `ConversationModel` 读 spinner / compact_progress / task_status。

### Step 5.2: StatusViewAssembler 改读 ConversationModel

`StatusViewAssembler::assemble_status_view` 入参从 `runtime: &RuntimeModel` 改为 `conversation: &ConversationModel`。

### Step 5.3: adapter 层适配

`adapter/status_widget.rs`、`adapter/live_status_widget.rs` 中对 RuntimeModel 的引用改为 ConversationModel。

### Step 5.4: app 层引用适配

`app.rs`、`app/runtime.rs`、`app/run_loop.rs`、`app/slash.rs`、`app/update/*` 中所有 `model.runtime.xxx` 改为 `model.conversation.xxx`。

**验证**: `cargo check -p cli` + `cargo test -p cli` + `cargo clippy -p cli`

---

## Phase 6: 清理与文档

### Step 6.1: 删除 model/runtime/ 目录

确认 `model/runtime/` 下无残留引用后删除目录。

### Step 6.2: 更新设计文档

更新 `docs/design/04-tui-design.md`：删除 Runtime Model 小节，更新 Conversation Model / Intent / Change 描述。

### Step 6.3: 全量验证

```bash
cargo fmt --check
cargo clippy -p cli -- -D warnings
cargo test -p cli
```

### Step 6.4: 架构守卫检查

确认 `.agents/hooks/` 中的 guard 脚本不引用已删除的路径。

---

## 风险

1. **改动面大**：~100+ 文件引用 RuntimeModel/RuntimeIntent。需要严格按 phase 推进，每步编译通过。
2. **测试适配量大**：大量测试直接构造 RuntimeModel / RuntimeIntent，需要逐个改为 ConversationModel / ConversationIntent。
3. **Intent trait 分发是范式变更**：所有现有 intent 测试需要适配新 struct 结构。
4. **ToolResult remaining 时序**：依赖同批次 intent 顺序 apply，需确保 root_reducer 中 `for intent in mapping.conversation` 循环保持顺序。
