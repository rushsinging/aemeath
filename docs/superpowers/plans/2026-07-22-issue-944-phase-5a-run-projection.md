# #944 阶段 5A：AgentRunState 消费契约实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 #944 建立 TUI-owned `AgentRunState` 生命周期状态机，使 #943 能将 SDK lifecycle DTO 无损映射为既定的 TUI Intent，而不让交互命令结果越权改变一次 Agent 运行状态。

**Architecture:** `ConversationModel` 保存按 `UiRunId` 索引的 `AgentRunState` 与有序 `AgentRunStepState`。它们是 Runtime 权威 lifecycle event 在 TUI 中的状态投影，但类型名描述业务对象而非实现方式。运行时 ACL 仍不改动：本阶段仅新增纯值 lifecycle Intent、reducer Change 与不变量；未来 #943 将其 TUI-owned DTO 映射为这些 Intent。Interaction confirm/cancel result 继续只改变 InteractionState，运行恢复/取消/终结仅接受 Runtime lifecycle Intent。

**Tech Stack:** Rust、现有 TUI `ConversationIntent → ConversationChange → root_reducer`、单元与架构测试。

---

## 范围与退役边界

### 本阶段建立

- `UiRunId` 下的 `AgentRunState`：状态为 `Running`、`AwaitingUser`、`Cancelling`、`Cancelled`、`Completed`、`Failed`。`Created` 不作为 TUI 状态保存：当前 Runtime Published Language 的首个可观察生命周期事实是 `RunStarted`，其直接投影为 `Running`。
- `AgentRunStepState`：拥有稳定 step id、状态与可选 tool reference；按 Runtime 输入顺序保存。
- 纯值 lifecycle Intent：Run started / awaiting user / resumed / cancelling / cancelled / completed / failed，及 step started / completed / cancellation requested / cancelled。
- `ConversationChange`：记录已应用的 Run / step lifecycle；root reducer 只据 Change 标脏 output/status，不产生 AgentClient 命令。
- L1 transition、L2 invariant、interaction-result-does-not-transition-run 测试。

### 本阶段明确不做

- **不改** `effect/session/processing/event_mapping.rs` 的 SDK `ChatEvent` match；#943 完成 TUI-owned ACL 后才将 DTO 映射为本阶段 Intent。
- **不删** `RunCancelled` legacy UiEvent、ChatStatus、spinner、`update_ui`、AskUser sender 或 registry；这些是后续 5B/最终退役项。
- **不新增** SDK type、sender、channel、AgentClient 或 I/O 到 Model、Intent、Change、Coordinator。
- **不让** `InteractionReplyAccepted`、`InteractionCancelAccepted` 或 rejection 改变 `AgentRunState`；只有 Runtime lifecycle Intent 能转换运行 phase。

## 文件结构

- Modify: `apps/cli/src/tui/model/conversation/interaction.rs` — 复用 `UiRunId`，新增 TUI-owned run / step projection 值类型及只读 accessor。
- Modify: `apps/cli/src/tui/model/conversation/model.rs` — 保存 `AgentRunState` collection，提供 package-private mutation helpers。
- Modify: `apps/cli/src/tui/model/conversation/intent.rs` — 定义 lifecycle intent structs 与 `ConversationIntent` variants。
- Modify: `apps/cli/src/tui/model/conversation/intent_impls.rs` — 将 lifecycle intent 以同一 reducer transaction 应用到运行状态。
- Modify: `apps/cli/src/tui/model/conversation/change.rs` — 定义 Agent run / step lifecycle changes。
- Modify: `apps/cli/src/tui/update/root_reducer.rs` — 将 run lifecycle change 映射为 output/status dirty；不生成 effect。
- Modify: `apps/cli/src/tui/model/conversation/interaction_tests.rs` — L1 phase transition 与交互结果隔离测试。
- Create: `apps/cli/src/tui/model/conversation/agent_run_state_tests.rs` — L2 ordering、identity、terminal-invariant 测试。
- Modify: `apps/cli/src/tui/model/conversation.rs` — 注册新增测试模块（如当前模块结构需要）。
- Modify: `apps/cli/src/tui/architecture_tests.rs` — 静态门禁：run model 不依赖 sdk/sender/channel/AgentClient，旧 SDK mapper 仍不承担本阶段状态机。
- Modify: `docs/design/03-engineering/03-migration-governance.md` — 记录 5A 的完成与 #943 接线责任。

### Task 1: 写 AgentRunState Red 测试

**Files:**
- Modify: `apps/cli/src/tui/model/conversation/interaction_tests.rs`
- Create: `apps/cli/src/tui/model/conversation/agent_run_state_tests.rs`

- [ ] **Step 1: 写状态迁移失败测试**

测试 `RunStarted → RunAwaitingUser → RunResumed → RunCancelling → RunCancelled`，断言每一步只接受同 run id、状态严格递进；终态 `Cancelled` 后 `RunResumed` 不得回退。

- [ ] **Step 2: 运行 Red 测试**

Run: `cargo test -p cli tui::model::conversation::agent_run_state -- --nocapture`

Expected: FAIL，缺少 lifecycle Intent / `AgentRunState` 类型或 accessor。

- [ ] **Step 3: 写 interaction-result 隔离失败测试**

在 `AwaitingUser` run 上执行 `InteractionReplyAccepted` 与 `InteractionCancelAccepted`；断言 run 仍为 `AwaitingUser`。仅 `RunResumed` 能回到 `Running`。

- [ ] **Step 4: 运行 Red 测试**

Run: `cargo test -p cli tui::model::conversation::interaction -- --nocapture`

Expected: FAIL，缺少 run lifecycle 或 run 状态被错误变更。

### Task 2: 实现最小 TUI-owned AgentRunState

**Files:**
- Modify: `apps/cli/src/tui/model/conversation/interaction.rs`
- Modify: `apps/cli/src/tui/model/conversation/model.rs`
- Modify: `apps/cli/src/tui/model/conversation/intent.rs`
- Modify: `apps/cli/src/tui/model/conversation/intent_impls.rs`
- Modify: `apps/cli/src/tui/model/conversation/change.rs`

- [ ] **Step 1: 定义纯值 projection 和 lifecycle intents**

`AgentRunState` 仅包含 TUI-owned `UiRunId`、phase、按序 `AgentRunStepState`；不出现 `sdk::`、sender、channel、AgentClient 或 runtime handle。重复 `RunStarted` 不覆盖已存在状态；未知 run 的后续事件产生结构化忽略 change，而非创建隐式运行。

- [ ] **Step 2: 以最小 reducer mutation 通过 L1 测试**

实现合法转换：

- `RunStarted`：创建状态并直接投影为 `Running`；
- `Running → AwaitingUser`：RunAwaitingUser；
- `AwaitingUser → Running`：RunResumed；
- `Running | AwaitingUser → Cancelling`：RunCancelling；
- `Cancelling → Cancelled`：RunCancelled；
- `Running → Completed | Failed`：对应 Runtime terminal intent。

终态拒绝所有回退；reducer 仅发 Change。

- [ ] **Step 3: 运行 L1 测试至 Green**

Run: `cargo test -p cli tui::model::conversation::agent_run_state -- --nocapture && cargo test -p cli tui::model::conversation::interaction -- --nocapture`

Expected: PASS。

### Task 3: 追加 AgentRunStepState 与 invariant 测试

**Files:**
- Modify: `apps/cli/src/tui/model/conversation/interaction.rs`
- Modify: `apps/cli/src/tui/model/conversation/intent.rs`
- Modify: `apps/cli/src/tui/model/conversation/intent_impls.rs`
- Modify: `apps/cli/src/tui/model/conversation/change.rs`
- Modify: `apps/cli/src/tui/model/conversation/agent_run_state_tests.rs`

- [ ] **Step 1: 写 step 顺序/identity Red 测试**

同一 run 连续接收两条 `RunStepStarted`，断言 insertion order 保持；同 step id 的 complete 更新原 step，不追加第二条；未知 step complete 产生 ignored change；不同 run 的同名 step 不串扰。

- [ ] **Step 2: 运行 Red 测试**

Run: `cargo test -p cli tui::model::conversation::agent_run_state -- --nocapture`

Expected: FAIL，缺少 `AgentRunStepState` / 更新规则。

- [ ] **Step 3: 实现最小 AgentRunStepState**

按 run id 查找，按 step id 原位更新；禁止从 timeline 或 tool block 推导 / 伪造运行步骤。

- [ ] **Step 4: 运行 Green 测试**

Run: `cargo test -p cli tui::model::conversation::agent_run_state -- --nocapture`

Expected: PASS。

### Task 4: 接入 reducer dirty 与边界守卫

**Files:**
- Modify: `apps/cli/src/tui/update/root_reducer.rs`
- Modify: `apps/cli/src/tui/update/root_reducer_intent_tests.rs`
- Modify: `apps/cli/src/tui/architecture_tests.rs`

- [ ] **Step 1: 写 reducer Red 测试**

从 `AgentIntent::Conversation(RunStarted)` 断言 output/status dirty 与唯一 `RequestRender`；断言 effects 中没有 command effect。对 ignored lifecycle transition 断言不脏、不 render。

- [ ] **Step 2: 运行 Red 测试**

Run: `cargo test -p cli tui::update::root_reducer -- --nocapture`

Expected: FAIL，Run changes 尚未映射 dirty。

- [ ] **Step 3: 实现 Change→dirty 映射**

只把有效 run / step lifecycle Change 归类为 output/status dirty；Coordinator 不为这些 Change 生成 effect。

- [ ] **Step 4: 增加架构门禁**

静态读取 run model / intent / change production source，断言零 `sdk::`、`oneshot::Sender`、`tokio::sync`、`AgentClient`、`.await`、`spawn`；断言 processing mapper 仍将 Run lifecycle 折叠，避免本阶段偷接 SDK ACL。

- [ ] **Step 5: 运行 Green 测试**

Run: `cargo test -p cli tui::update::root_reducer -- --nocapture && cargo test -p cli tui::architecture_tests -- --nocapture`

Expected: PASS。

## 实施结果（2026-07-22）

- `AgentRunState` 已按 `UiRunId` 保存 TUI-owned lifecycle phase；运行、等待用户、取消中、已取消、已完成、失败均只经 Runtime lifecycle Intent 转换。
- `AgentRunStepState` 已按稳定 step id 保存，并在同一 run 内保持 Runtime 输入顺序；完成事件原位更新，未知 run/step 与重复开始均不创建隐式状态。
- interaction command result 仅影响 interaction block，无法恢复、取消或终结对应运行。
- reducer 将有效 run/step Change 标记为 output+status dirty，并只产生一次 render request，不产生 command Effect；无效 transition 不标脏。
- 静态门禁已锁定 run model / lifecycle declarations 的 TUI-owned 纯值边界，并确认 #943 前 SDK mapper 尚未接线。
- 验证通过：AgentRunState、root reducer、architecture 定向测试与 `cargo check -p cli`。完整阶段门禁见本计划的 Task 5 Step 2。

### Task 5: 文档回写与验收

**Files:**
- Modify: `docs/design/03-engineering/03-migration-governance.md`
- Modify: `docs/superpowers/plans/2026-07-22-issue-944-phase-5a-run-projection.md`

- [ ] **Step 1: 记录实现结果与 #943 接线责任**

在 Governance O6 说明：#944 已提供 TUI-owned run lifecycle Intent/Change/state machine；#943 只负责 SDK DTO 到这些 Intent 的无损 ACL；legacy mapper / sender / spinner 仍未退役。

- [ ] **Step 2: 执行完整验收**

Run:

```bash
cargo fmt --all -- --check
cargo test -p cli tui::model::conversation::agent_run_state -- --nocapture
cargo test -p cli tui::model::conversation::interaction -- --nocapture
cargo test -p cli tui::update::root_reducer -- --nocapture
cargo test -p cli tui::architecture_tests -- --nocapture
cargo check -p cli
PATH="/opt/homebrew/bin:$PATH" bash .agents/hooks/check-tui-tea-purity.sh
PATH="/opt/homebrew/bin:$PATH" bash .agents/hooks/check-tui-effect-boundary.sh
PATH="/opt/homebrew/bin:$PATH" bash .agents/hooks/check-tui-model-view-boundaries.sh
git diff --check
```

Expected: all commands exit 0. Run `cargo clippy -p cli --all-targets -- -D warnings` separately; if existing known lint blockers remain, record them without expanding scope.
