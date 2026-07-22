# Issue #944 阶段 4D 实施计划：sender-free Interaction 消费面

> 对应 Issue：[ #944](https://github.com/rushsinging/aemeath/issues/944)。
> 前置：4A 已收口 ACL Effect；4B ConfigProvider 与 4C WorkspaceProvider 已完成。#943 被 #944 阻塞，本计划先提供其可消费的 TUI Interaction Intent / Effect 边界。
> 本阶段只建立 TUI 的 sender-free Interaction Model、Intent → Change → Effect → result Intent 闭环；**不接入 SDK ChatEvent，不删除 legacy AskUser sender 路径**。

## Goal

建立由 TUI-owned Interaction DTO 驱动的通用交互消费面。它支持 UserQuestions、ToolApproval、PlanApproval、HardPause 四种 body，且确认/取消只能通过 Coordinator 派生的 Effect 调用 `AgentClient`，再以 result Intent 回到 root reducer。

## Architecture

```text
TUI-owned InteractionRequest
  → ConversationIntent::ShowInteraction
  → ConversationModel::InteractionState
  → ConversationChange::InteractionReplyRequested
  → Coordinator::effects_for_conversation_change
  → Effect::ReplyInteraction
  → EffectExecutor::AgentClient::reply_interaction
  → ConversationIntent::InteractionReplyAccepted / Rejected
  → root_reducer
```

Interaction 是 Conversation Context 内单一 active projection：同一时刻第二个 request 不得覆盖第一个，而是产生结构化 conflict Diagnostic。Interaction command 的 Accepted / Rejected result 只改变 Interaction phase，**NEVER** 迁移 Run 状态；Run 恢复、进入 Cancelling 和终态仍等待 Runtime 权威 lifecycle event。

## Tech Stack

Rust、现有 `TuiModel` / `AgentIntent` / root reducer / Coordinator / `Effect` / `sdk::AgentClient` 契约；使用 recording fake 验证 AgentClient 命令。

---

## 范围与依赖

### 本阶段建立

- TUI-owned Interaction request/body/reply/draft/phase 类型；类型定义不引用 `sdk::*`。
- Conversation 内单一 `InteractionState` 及只读 projection。
- 展示、编辑、确认、取消、reply/cancel outcome 的 Conversation Intent 与 Change。
- Change → `Effect::ReplyInteraction` / `Effect::CancelInteraction` 的纯 Coordinator 映射。
- EffectExecutor 通过 `App.agent_client` 调用 Runtime-owned `AgentClient` 命令，并回灌 result Intent。
- L0、L1、L2、L3 分层测试。

### 本阶段明确不做

- 不改 `sdk::ChatEvent` 或新增 `UiEvent::InteractionRequested`；该第一层 DTO ACL 接线属于 #943。
- 不删除或改写 `UiEvent::AskUserBatch`、`AskUserState.reply_tx`、`ask_user_reply_tx`、`update_ui` 的 legacy AskUser sender 路径；这在 #943 / #1246 生产输入切线后退役。
- 不将 Interaction 接到现有键盘 AskUser 处理；4D 只提供通用 Intent 入口，#943 到位后才加输入适配。
- 不实现 RunProjection/lifecycle 状态机；Interaction result 只维护自身 phase。
- 不实现 Workspace metadata Effect；留给 4E。

### 外部依赖语义

- `packages/sdk/src/interaction.rs` 已定义 Runtime-owned command 语义：`Accepted`、`NotFound`、`AlreadyCompleted`、`InvalidReply`、`RunCancelling`。
- `packages/sdk/src/client.rs` 已定义同步 `AgentClient::reply_interaction` / `cancel_interaction`。
- Runtime `InteractionBridge` 已持有 waiter 与 continuation；TUI 只传纯值 command，**NEVER** 持有 sender、waiter、registry 或 continuation。
- #1246 接线前 AgentClient 可能仍返回默认 `NotFound`；TUI 必须将其建模为失败而非将本地 Interaction 伪标为完成。

## 文件结构

- Create: `apps/cli/src/tui/model/conversation/interaction.rs`
  - TUI-owned `InteractionRequest`、四种 `InteractionBody`、`InteractionDraft`、`InteractionReply`、`InteractionPhase`、`InteractionCommandFailure`；不 import `sdk`。
- Create: `apps/cli/src/tui/model/conversation/interaction_tests.rs`
  - L1 Interaction 状态机、重复请求冲突与结果处理测试。
- Modify: `apps/cli/src/tui/model/conversation/model.rs`
  - 增加私有 `active_interaction: Option<InteractionState>` 与只读 accessor。
- Modify: `apps/cli/src/tui/model/conversation/intent.rs`
  - 增加 Interaction Intent 及 `ConversationIntent` 变体。
- Modify: `apps/cli/src/tui/model/conversation/intent_impls.rs`
  - 将 Interaction Intent 分派给 `ConversationModel`。
- Modify: `apps/cli/src/tui/model/conversation/change.rs`
  - 增加 Interaction Change。
- Modify: `apps/cli/src/tui/update/coordinator.rs`
  - Change → Effect 纯映射。
- Modify: `apps/cli/src/tui/effect/effect.rs`
  - 增加纯值 reply/cancel Effect，使用 TUI-owned interaction 类型。
- Modify: `apps/cli/src/tui/effect/executor.rs`
  - 仅在 executor 做 TUI-owned ↔ SDK command 转换、调用 `AgentClient`、回灌 AgentIntent。
- Create: `apps/cli/src/tui/effect/executor_interaction_tests.rs`
  - L3 recording fake 与命令/outcome 验证。
- Modify: `apps/cli/src/tui/update/root_reducer_intent_tests.rs`
  - L2 root reducer 的 Interaction Change/dirty 验证。
- Modify: `apps/cli/src/tui/architecture_tests.rs`
  - L0 防止 TUI-owned Interaction 类型、Model、Intent 出现 SDK/sender/waiter/registry。

## 交付任务

### Task 1：冻结 TUI-owned Interaction 值类型与失败测试

**Files:**
- Create: `apps/cli/src/tui/model/conversation/interaction.rs`
- Create: `apps/cli/src/tui/model/conversation/interaction_tests.rs`
- Modify: `apps/cli/src/tui/model/conversation/model.rs`

- [ ] **Step 1: 写 Interaction 状态机失败测试**

覆盖：

- `show` 接收第一份 request 后进入 `Collecting`；
- 四种 body 都有对应 typed draft；
- 第二个不同 request 不覆盖 active request，而产生 conflict；
- `confirm` 仅进入 `ReplyPending` 并要求 reply Change；
- `cancel` 仅进入 `CancelPending` 并要求 cancel Change；
- `InvalidReply` 回到原可编辑 phase，保留 draft；
- `Accepted` 清除 active interaction；
- reply/cancel 结果不改写 Conversation 的 chat/run runtime 状态。

测试中使用 TUI-owned fixture，避免在 model 直接构造 SDK interaction 类型：

```rust
let request = InteractionRequest {
    request_id: UiInteractionRequestId::from("018f0000-0000-7000-8000-000000000001"),
    run_id: UiRunId::from("018f0000-0000-7000-8000-000000000002"),
    body: InteractionBody::ToolApproval(UiApprovalPrompt {
        title: "Bash".to_string(),
        detail: "rm -rf build".to_string(),
        risk: UiRiskLevel::High,
    }),
};
```

- [ ] **Step 2: 运行测试，确认因类型/方法缺失失败**

Run: `cargo test -p cli tui::model::conversation::interaction -- --nocapture`

Expected: 编译失败，提示 `interaction` 模块、`InteractionRequest` 或 `show_interaction` 尚不存在。

- [ ] **Step 3: 实现最小 TUI-owned 类型和状态槽**

在 `interaction.rs` 定义仅含 `String`、`Vec`、bool 和 TUI-owned enum/newtype 的纯值模型：

```rust
pub(crate) enum InteractionPhase {
    Collecting,
    Confirming,
    ReplyPending,
    CancelPending,
}

pub(crate) struct InteractionState {
    request: InteractionRequest,
    draft: InteractionDraft,
    phase: InteractionPhase,
}
```

在 `ConversationModel` 增加私有 `active_interaction: Option<InteractionState>` 和只读 `active_interaction()` projection；**不得**使用 `sdk::InteractionRequestId`、`sdk::InteractionReply` 或任何 `tokio` 类型。

- [ ] **Step 4: 运行状态机测试，确认通过**

Run: `cargo test -p cli tui::model::conversation::interaction -- --nocapture`

Expected: PASS；四 body、重复请求、pending/result phase 均有语义断言。

### Task 2：以 Intent / Change 收口 Interaction 状态迁移

**Files:**
- Modify: `apps/cli/src/tui/model/conversation/intent.rs`
- Modify: `apps/cli/src/tui/model/conversation/intent_impls.rs`
- Modify: `apps/cli/src/tui/model/conversation/change.rs`
- Modify: `apps/cli/src/tui/update/root_reducer_intent_tests.rs`

- [ ] **Step 1: 写 root reducer 失败测试**

新增测试确认：

```rust
let result = reduce_intent(
    &mut model,
    AgentIntent::Conversation(ConversationIntent::ConfirmInteraction(
        ConfirmInteraction { request_id: request_id.clone() },
    )),
);
assert!(result.effects.is_empty());
assert!(result.dirty.output);
assert_eq!(
    model.conversation.active_interaction().unwrap().phase(),
    InteractionPhase::ReplyPending,
);
```

并直接检查 Conversation Change 包含 `InteractionReplyRequested { request_id, reply }`，不检查或改变任何 Run status。

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p cli tui::update::root_reducer::intent_tests -- --nocapture`

Expected: FAIL，因为 `ConfirmInteraction`、`InteractionReplyRequested` 尚不存在。

- [ ] **Step 3: 添加 Intent / Change 与 trait 分派**

定义至少以下 Intent：

```rust
ShowInteraction(ShowInteraction)
UpdateInteractionDraft(UpdateInteractionDraft)
ConfirmInteraction(ConfirmInteraction)
CancelInteraction(CancelInteraction)
InteractionReplyAccepted(InteractionReplyAccepted)
InteractionReplyRejected(InteractionReplyRejected)
InteractionCancelAccepted(InteractionCancelAccepted)
InteractionCancelRejected(InteractionCancelRejected)
```

定义至少以下 Change：

```rust
InteractionShown { request_id: UiInteractionRequestId }
InteractionUpdated { request_id: UiInteractionRequestId }
InteractionReplyRequested { request_id: UiInteractionRequestId, reply: UiInteractionReply }
InteractionCancelRequested { request_id: UiInteractionRequestId }
InteractionCompleted { request_id: UiInteractionRequestId }
InteractionCommandRejected { request_id: UiInteractionRequestId, failure: InteractionCommandFailure }
InteractionConflict { active_request_id: UiInteractionRequestId, received_request_id: UiInteractionRequestId }
```

`ConfirmInteraction` / `CancelInteraction` 必须只改变 Interaction phase 并产生 request Change；不得直接调用 Coordinator、Effect、AgentClient 或改变 runtime chat phase。

- [ ] **Step 4: 运行 reducer 测试，确认通过**

Run: `cargo test -p cli tui::update::root_reducer::intent_tests -- --nocapture`

Expected: PASS；输出 dirty 由 Interaction Change 产生，result Intent 只改变 interaction。

### Task 3：由 Coordinator 将 command Change 映射为纯 Effect

**Files:**
- Modify: `apps/cli/src/tui/effect/effect.rs`
- Modify: `apps/cli/src/tui/update/coordinator.rs`
- Add/modify: coordinator 对应 `*_tests.rs`

- [ ] **Step 1: 写 Change → Effect 的失败测试**

为 reply 和 cancel 各写一条测试：

```rust
let effects = effects_for_conversation_change(
    &ConversationChange::InteractionReplyRequested {
        request_id: request_id.clone(),
        reply: UiInteractionReply::HardPauseContinue,
    },
);
assert_eq!(effects, vec![Effect::ReplyInteraction { request_id, reply }]);
```

`InteractionShown`、`InteractionUpdated`、`InteractionCompleted`、`InteractionCommandRejected` 不得产生 interaction command Effect。

- [ ] **Step 2: 运行 Coordinator 测试，确认失败**

Run: `cargo test -p cli tui::update::coordinator -- --nocapture`

Expected: FAIL，因为 `Effect::ReplyInteraction` / `CancelInteraction` 尚不存在。

- [ ] **Step 3: 定义纯值 Effect 并实现映射**

Effect 只能携带 TUI-owned request id / reply / cancel reason：

```rust
ReplyInteraction {
    request_id: UiInteractionRequestId,
    reply: UiInteractionReply,
},
CancelInteraction {
    request_id: UiInteractionRequestId,
    reason: UiInteractionCancelReason,
},
```

Coordinator 只做 match 映射；不 import `sdk`，不执行 I/O，不读 `App`。

- [ ] **Step 4: 运行 Coordinator 测试，确认通过**

Run: `cargo test -p cli tui::update::coordinator -- --nocapture`

Expected: PASS；每种 command Change 恰好生成一个对应 Effect。

### Task 4：EffectExecutor 调 AgentClient 并回灌 result Intent

**Files:**
- Modify: `apps/cli/src/tui/effect/executor.rs`
- Create: `apps/cli/src/tui/effect/executor_interaction_tests.rs`
- Modify: `apps/cli/src/tui/effect/effect.rs`

- [ ] **Step 1: 写 executor 失败测试与 recording fake**

在独立测试文件定义 `RecordingInteractionClient`，实现 `sdk::AgentClient`，记录 `reply_interaction` / `cancel_interaction` 的 SDK 参数并配置预设 `InteractionCommandOutcome`。

覆盖：

- reply Effect 将 TUI-owned id/reply 转换为正确 body-specific SDK command，且只调用一次；
- cancel Effect 固定传 `sdk::InteractionCancelReason::UserCancelled`；
- `Accepted` 回灌 accepted Intent；
- `InvalidReply` 回灌 rejected Intent，且保留 draft；
- `NotFound`、`AlreadyCompleted`、`RunCancelling` 回灌可展示且可诊断的 rejected Intent；
- `agent_client == None` 不可当作成功，必须回灌 failure Intent。

- [ ] **Step 2: 运行 executor 测试，确认失败**

Run: `cargo test -p cli tui::effect::executor::interaction -- --nocapture`

Expected: FAIL，因为 executor 尚不能执行 Interaction Effect 或把 outcome 转成 Intent。

- [ ] **Step 3: 在 executor 唯一执行 SDK 转换与命令调用**

`EffectExecutor` 的实现只能在 `executor.rs`：

1. 将 TUI-owned id/reply/cancel reason 转换为 SDK command 纯值；
2. 通过 `self.agent_client.as_ref()` 调用同步 API；
3. 完整 match `InteractionCommandOutcome`；
4. 通过 `self.apply_agent_intent(AgentIntent::Conversation(...))` 回灌；
5. 绝不直接写 `ConversationModel`，绝不发送 sender / channel。

`InteractionCommandOutcome::InvalidReply` 映射 `InteractionReplyRejected`；其它非 Accepted outcome 映射 `InteractionCancelRejected` 或 reply 的通用 `InteractionReplyRejected`，保留 request id 与中文 failure 描述。

- [ ] **Step 4: 运行 executor 测试，确认通过**

Run: `cargo test -p cli tui::effect::executor::interaction -- --nocapture`

Expected: PASS；recording fake 验证每条 command 参数与 outcome result Intent。

### Task 5：架构门禁、legacy 隔离与阶段验收

**Files:**
- Modify: `apps/cli/src/tui/architecture_tests.rs`
- Modify: `docs/design/03-engineering/03-migration-governance.md`
- Modify: `docs/superpowers/plans/2026-07-22-issue-944-phase-4d-interaction-consumer.md`

- [ ] **Step 1: 写 L0 boundary 失败断言**

架构测试读取新 Interaction model / intent / change / coordinator 源文件并断言：

- `model/conversation/interaction.rs`、`intent.rs`、`change.rs` 不含 `sdk::`、`oneshot::Sender`、`tokio::sync`、`PendingInteraction`、`InteractionBridge`、`Registry`；
- `coordinator.rs` 不含 `AgentClient`、`await`、`spawn`；
- 只有 `effect/executor.rs` 可出现 Interaction SDK command 转换；
- legacy `AskUserBatch` sender 路径仍存在但不被新 Interaction 模型引用。

- [ ] **Step 2: 运行架构测试，确认先失败后通过**

Run: `cargo test -p cli tui::architecture_tests -- --nocapture`

Expected before implementation: FAIL；完成 Task 1–4 后: PASS。

- [ ] **Step 3: 回写 Migration Governance Current → Target 证据**

更新 O6 / TUI-2、TUI-3：标记 #944 已具备 sender-free Interaction consumption / command seam，且明确 legacy sender 删除仍等待 #943 DTO 输入与 #1246 production suspension 切线。

- [ ] **Step 4: 执行阶段验收**

Run:

```bash
cargo test -p cli tui::model::conversation::interaction -- --nocapture
cargo test -p cli tui::update::root_reducer -- --nocapture
cargo test -p cli tui::update::coordinator -- --nocapture
cargo test -p cli tui::effect::executor::interaction -- --nocapture
cargo test -p cli tui::architecture_tests -- --nocapture
cargo fmt --all -- --check
cargo clippy -p cli --all-targets -- -D warnings
PATH="/opt/homebrew/bin:$PATH" bash .agents/hooks/check-tui-tea-purity.sh
PATH="/opt/homebrew/bin:$PATH" bash .agents/hooks/check-tui-effect-boundary.sh
PATH="/opt/homebrew/bin:$PATH" bash .agents/hooks/check-tui-model-view-boundaries.sh
git diff --check
```

Expected: 全部通过；若 `AgentClient` 生产实现尚未覆盖 interaction command，测试 fake 仍可验证 TUI 边界，但 #944 不得宣称 legacy sender 已退役。

## 实施结果（2026-07-22）

- 已建立 TUI-owned `InteractionRequest`、四种 `InteractionBody`、typed draft / reply / phase 以及单 active request conflict。
- 已建立 `ShowInteraction`、draft 更新、confirm/cancel、reply/cancel accepted/rejected Intent 与对应 Change；result Intent 不写入 Conversation runtime。
- Coordinator 已将 reply/cancel Change 纯映射为 `Effect::ReplyInteraction` / `Effect::CancelInteraction`；executor 是唯一 SDK command 转换与 `AgentClient` 调用点。
- executor 对 request id 使用 UUIDv7 严格解析；格式无效或 client 缺失时回灌可恢复失败 Intent，不调用 Runtime command。
- 旧 `AskUserBatch.reply_tx` 路径未改变；等待 #943 纯 DTO ACL 和 #1246 production suspension 切线后退役。
- 验证：Interaction model/reducer/coordinator/executor/architecture 定向测试、`cargo check -p cli`、fmt、TUI TEA/effect/model-view guards、`git diff --check` 通过；`cargo clippy -p cli --all-targets -- -D warnings` 仍被已有的非本阶段 lint 阻断（`ChatState`/`RuntimeState` 的 derive 建议、ConfigProvider variant 命名）。

## 退役计划与退出门槛

| 项目 | 4D 处理 | 后续责任 | 退出证据 |
|---|---|---|---|
| TUI-owned Interaction state / command seam | 本阶段建立 | #944 | 四 body 分层测试和 L0 guard 通过 |
| SDK → UiEvent DTO 转换 | 不做 | #943 | 无损 DTO table test，TUI event 零 SDK DTO |
| Runtime Main suspension production routing | 不做 | #1246 | Runtime waiter/continuation 生产可达 |
| legacy `AskUserBatch.reply_tx` / InputState sender | 隔离但保留 | #944 后续退役（依赖 #943/#1246） | 新 Interaction 输入接线后零引用 |
| `update_ui` legacy 双路径 | 不做 | #944 后续 retirement | Interaction 转为 reducer-only 后删除 |
| Run lifecycle projection | 不做 | #944 后续阶段 | 仅 Runtime lifecycle Intent 能迁移 Run |

## 计划自检

- 覆盖 #944 Interaction 目标：四 body、sender-free state、Change → Effect、AgentClient outcome result Intent、result 不推进 Run。
- 未越界到 #943：未定义 SDK ChatEvent → UiEvent converter，未让 TUI Interaction 类型依赖 SDK。
- 未越界到 #1246：未触及 Runtime waiter/continuation 或 Main suspension 接线。
- 保留 legacy 的原因明确且可验证：旧 AskUser sender 仍只在 legacy event / app state / key handler 路径，不能被新 Interaction 类型引用。
- 每个新增核心行为都要求先建立失败测试，并覆盖 L0–L3 相邻层。
