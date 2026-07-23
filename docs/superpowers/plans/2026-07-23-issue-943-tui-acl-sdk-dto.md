# Issue #943 实施计划：全量 TUI ACL 与 SDK DTO 隔离

> 对应 Issue：[#943](https://github.com/rushsinging/aemeath/issues/943)，父 Issue：#860。
> 基线：`origin/main` `aa87bbdd82c918888e52a4a3be19ae3ebd64d387`（#1357 已合入）。
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans task-by-task.

**Goal:** 以一次完整迁移建立唯一 `sdk::ChatEvent → TuiRuntimeEvent → AgentIntent / App update` 链路。`TuiRuntimeEvent` 是 processing mpsc 通道唯一消息类型、完全由 TUI-owned 纯值组成；`UiEvent` 及其 SDK DTO 直接持有路径被删除或降为不接收 Runtime stream 的本地 UI 事件。

**Architecture:** `adapter/event_mapping.rs` 是唯一允许穷尽匹配 `sdk::ChatEvent` 的生产模块。它将每个 SDK 变体转换为 `adapter/tui_runtime_event.rs` 的纯值 `TuiRuntimeEvent`。processing 只发送 `TuiRuntimeEvent`；App 对 `TuiRuntimeEvent` 先经 `adapter/agent_event.rs` 产生 #944 已有 `AgentIntent`，再处理确属本地 UI 的纯值效果。不得有 SDK payload 直达 `app/update`，不得有 `LegacyPassthrough`、空字符串或 wildcard 作为未实现事件的类型擦除。未来其他 client 必须定义各自的 `*RuntimeEvent` 投影；跨 client 语义只提升至 SDK Published Language。

**Tech Stack:** Rust、SDK Published Language、#944 `AgentIntent → root_reducer → Change/Effect`、L0–L4 测试。

---

## 不可变边界

- #944 consumer core 已合入：Interaction、Workspace metadata、AgentRunState / AgentRunStepState 均已有纯值 Intent 与 reducer。
- #1246 仅依赖 #943；本 Issue完成后 Runtime 才切 Main suspension。
- **#944 5B 已并入本 Issue的单一 PR**：本 PR 物理删除 legacy `AskUserBatch.reply_tx`、InputState sender、`UiEvent → update_ui` Runtime 双路径、旧 ChatStatus/spinner 第二状态源和 TUI Model/View/Render 的 SDK DTO 直接持有。#944 保留 consumer core 与验收追踪，不再单独创建 5B PR。
- Runtime Main suspension 的生产切换仍归 #1246；本 Issue只删除旧 sender bridge，并消费 #1246 已发布的 `InteractionRequested`。
- 本 Issue不新建 Runtime ID、waiter、registry、AgentClient command，不让 ACL 改变 Model。
- 零新增 guard exception / allowlist。

## 文件职责

- Create: `apps/cli/src/tui/adapter/runtime_view.rs` — 可复用纯值展示 DTO：message/content/image、hook、progress、task、config、session 等。
- Create: `apps/cli/src/tui/adapter/runtime_view_tests.rs` — runtime view 纯值转换与字段完整性测试。
- Create: `apps/cli/src/tui/adapter/tui_runtime_event.rs` — 所有 Runtime→TUI 所需的纯值事件、ID、run、interaction、workspace 类型。
- Create: `apps/cli/src/tui/adapter/tui_runtime_event_tests.rs` — 事件 DTO 纯值静态边界与构造测试。
- Create: `apps/cli/src/tui/adapter/event_mapping.rs` — 唯一 SDK ChatEvent 穷尽 converter。
- Create: `apps/cli/src/tui/adapter/event_mapping_tests.rs` — 所有 ChatEvent 变体的表驱动 L3 字段/identity 契约。
- Modify: `apps/cli/src/tui/adapter.rs` — 导出 ACL 模块。
- Modify: `apps/cli/src/tui/effect/session/processing.rs` 及其子模块 — mpsc sender、processing loop、logging 改为 `AclEvent`，删除 effect-side SDK→UiEvent mapper。
- Modify: `apps/cli/src/tui/adapter/agent_event.rs` / tests — 第二层仅匹配 `AclEvent`，显式产六 Context `AgentIntent`；不引用 SDK。
- Modify: `apps/cli/src/tui/app/event.rs`、`app/update.rs`、`app/update/ui_event.rs`、scenario harness — Runtime stream 入口改为 `AclEvent`；保留仅本地 UI 事件的纯值分支。
- Modify: `apps/cli/src/tui/model/**`、`view_assembler/**`、`render/**` 的事件消费点 — 改读 ACL DTO，禁止 SDK view 透传。
- Modify: `apps/cli/src/tui/architecture_tests.rs` 与 TUI guards — 唯一转换点与 SDK/sender/waiter/registry 越界规则。
- Modify: governance 与 Issue #943 — 回写完成证据、#1246 接线责任与 #944 5B 退役责任。

## Task 1：全量 AclEvent Red 表（L1/L3）

- [ ] 写 `tui_runtime_event_tests.rs` 静态 Red：生产 DTO source 零 `sdk::`、`oneshot::Sender`、`mpsc::Sender`、`AgentClient`、`PendingInteraction`、`Registry`。
- [ ] 写 `event_mapping_tests.rs` 表驱动 Red，覆盖每一个 `sdk::ChatEvent`：
  - token/thinking/block/tool start-update-result/model waiting/retry/usage；
  - message sync、queued/adopted、done/cancelled、clipboard、system/error；
  - 全部 Run/RunStep lifecycle 与 terminal event；
  - InteractionRequested 四 body、AskUserBatch 的 legacy bridge 边界；
  - agent progress、hook/hook message、workspace、config、session、task、reflection、model/context/command result；
  - 所有字段、run/step/request/input/tool/chat/turn identity 无损。
- [ ] Run：`cargo test -p cli tui::adapter::event_mapping -- --nocapture`
- [ ] Expected：FAIL，AclEvent/converter 尚不存在。

## Task 2：实现全量纯值 DTO 与唯一 converter（L1/L3 Green）

- [ ] 定义 `runtime_view` 的 message/content/image、hook、progress、task、config、session 等纯值 DTO；`TuiRuntimeEvent` 只聚合事件、ID、run、interaction、workspace。SDK ID 转为 TUI-owned wrapper/string，新 DTO 不直接持 SDK 类型。
- [ ] 所有 ChatMessage / content / image / hook / progress / task / config / session payload 改为 `runtime_view` 的 ACL-owned value DTO 或已安全的 primitive/serde_json value；不得透传 SDK struct。
- [ ] 定义 AclRunEvent / AclRunStepEvent / AclInteractionRequest / AclInteractionBody / AclWorkspaceSnapshot，保留 parent identity、context stack 与所有四 body 字段。
- [ ] 实现 `sdk_event_to_acl_event(event: sdk::ChatEvent) -> AclEvent` 的穷尽 match；converter 只解构/复制，**NEVER** 调 git、构造 reply、注册 continuation、生成 ID 或执行 I/O。
- [ ] `AskUserBatch` 不跨越 `TuiRuntimeEvent`：直到 #1246 切线前，只能在 processing 边界的临时 legacy resource bridge 中存在；本 PR 同时删除该 bridge、InputState sender 与旧 AskUser UI，禁止留下 sender 容器或兼容分支。
- [ ] processing mpsc 通道改为 `TuiRuntimeEvent`，删除旧 `effect/session/processing/event_mapping.rs` 的 SDK match。
- [ ] Run：`cargo test -p cli tui::adapter::event_mapping -- --nocapture`
- [ ] Expected：PASS。

## Task 3：全量第二层 ACL 映射（L1/L2）

- [ ] 先为每类 TuiRuntimeEvent 写 DTO→Intent Red：Conversation/Input/Diagnostic/Session/Config/Workspace 六个 Context 均使用 #944 `AgentIntent`。
- [ ] lifecycle 映射：Run Started/AwaitingUser/Resumed/Cancelling/Cancelled/Completed/Failed 与 step Started/Completed 映射至既有 ConversationIntent；不可再折叠为空消息。
- [ ] Interaction 四 body → `ShowInteraction`，保留 request/run identity；不创建 sender/registry。
- [ ] Workspace snapshot → `ApplySnapshot`；metadata 仍由 #944 Effect 生成；不含 branch/kind。
- [ ] Tool、progress、Hook、messages、usage、session、task、config 等映射为相应纯值 Intent；没有 Model consumer 的 DTO 必须映射为显式本地 UI 行为或结构化 diagnostic，禁止 default/wildcard 静默丢弃。
- [ ] `agent_event.rs` 零 `sdk::`、零 Effect、零 I/O。
- [ ] Run：

```bash
cargo test -p cli tui::adapter::agent_event -- --nocapture
cargo test -p cli tui::update::root_reducer -- --nocapture
```

- [ ] Expected：PASS。

## Task 4：App / View 全通道切换（L2/L4）

- [ ] 将 App processing receiver、TuiMsg、update 分发和 scenario harness 改为接收 TuiRuntimeEvent；仅 local keyboard/mouse/resize/timer 仍保持本地消息。
- [ ] 将所有仍读取 SDK event/view 的 TUI 消费点改为 ACL DTO；必要的格式化/渲染读取 ACL-owned 字段。
- [ ] 为 SDK event→AclEvent→Intent→reducer 增加相邻场景：四 body、RunCancelling→RunCancelled、workspace snapshot→metadata Effect、tool/progress/hook、queued/adopted identity。
- [ ] 验证 `InteractionRequested` 是唯一交互展示路径；legacy AskUser sender / UI 不得与新 DTO 双发，也不得在本 PR 后保留。

## Task 5：L0 Guard、治理与验收

- [ ] Guard：生产中 `sdk::ChatEvent` match 只允许 `adapter/event_mapping.rs`；TuiRuntimeEvent / agent mapper / model / view / render 零 SDK DTO、sender、waiter、registry；processing 零第二份 converter。
- [ ] Guard 负例：SDK DTO 写入 TuiRuntimeEvent、agent mapper 产生 Effect、converter 调 git、Run lifecycle 折叠空消息、额外 SDK ChatEvent match，均 exit 2。
- [ ] 更新 governance / #943：#943 完成全量 ACL；#1246 切 Main suspension；#944 5B 删除 legacy bridge。

## 验收

```bash
cargo fmt --all -- --check
cargo test -p cli tui::adapter::event_mapping -- --nocapture
cargo test -p cli tui::adapter::agent_event -- --nocapture
cargo test -p cli tui::update::root_reducer -- --nocapture
cargo test -p cli tui::architecture_tests -- --nocapture
cargo test -p sdk interaction -- --nocapture
cargo check -p cli -p sdk
PATH="/opt/homebrew/bin:$PATH" bash .agents/hooks/check-tui-model-view-boundaries.sh
PATH="/opt/homebrew/bin:$PATH" bash .agents/hooks/check-tui-effect-boundary.sh
PATH="/opt/homebrew/bin:$PATH" bash .agents/hooks/check-architecture-guards.sh --full
git diff --check
```

`cargo clippy -p cli -p sdk --all-targets -- -D warnings` 单独执行；若仍由 main 既有 lint 阻断，在 PR Test plan 如实记录，不扩展范围。
