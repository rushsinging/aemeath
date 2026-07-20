# Issue #944 实施计划：TUI TEA 核心与 Model / ViewState 分离

> 对应 Issue：[ #944](https://github.com/rushsinging/aemeath/issues/944)，父 Issue：#860。
> 开发前门禁：[`2026-07-20-issue-944-tea-preflight.md`](../specs/2026-07-20-issue-944-tea-preflight.md)。
> 本计划先建立 #944 的 TUI-owned Intent / Model / reducer 消费面；#943 随后接入 SDK→TUI ACL。

## 目标架构

```text
TuiMsg::Ui(UiEvent)                 # #943 之后为纯 TUI DTO
  → AgentEventMapper
  → Vec<AgentIntent>
  → root_reducer::reduce(model, intents)
  → Vec<ModelChange>
  → Coordinator::effects_for(changes)
  → Vec<Effect>
  → EffectExecutor
  → TuiMsg::Intent(result_intent)
```

`AgentIntent` 是六 Context 的闭合分发容器：Conversation、Input、Diagnostic、Session、Config、Workspace。root reducer 是唯一生产 mutation façade；Context model 的内部字段私有，只暴露只读 projection。

## 约束与非范围

- 本 Issue不转换 `sdk::ChatEvent`，不保留或删除任何 sender；#943 / #947 各自负责。
- 本 Issue不执行 Runtime waiter、Main suspension 或新 Run control；#878/#1246 负责。
- 本 Issue建立 `RequestRunCancellation` / Interaction command Effect 的纯结构和 result Intent；#945 实现唯一 CancelRun effect 的生产入口，#943 提供 SDK lifecycle DTO。
- 所有旧 `update_ui` 旁路的物理删除由 #947；过渡适配仅能调用 root reducer，禁止新增第二条 mutation 路径。

## 文件边界

- `apps/cli/src/tui/model/root.rs`：六个私有 Context 与只读 accessor。
- `apps/cli/src/tui/model/{conversation,input,diagnostic,session,config,workspace}/`：Context intent / change / state；不做 I/O。
- `apps/cli/src/tui/update/intent.rs`：`AgentIntent` 和 Context-tagged result Intent。
- `apps/cli/src/tui/update/root_reducer.rs`：唯一 Context dispatch，合并 `ModelChange`。
- `apps/cli/src/tui/update/coordinator.rs`：`ModelChange → Effect` 纯推导。
- `apps/cli/src/tui/effect/effect.rs`：纯 Effect 值；不持 sender / Runtime continuation。
- `apps/cli/src/tui/effect/executor.rs`：唯一 AgentClient reply/cancel 调用点，将 outcome 转为 `AgentIntent`。
- `apps/cli/src/tui/view_state.rs`：只保留 scroll / selection / collapse / animation / cache / dirty。
- `apps/cli/src/tui/architecture_tests.rs` 与 `.agents/hooks/check-tui-*.sh`：唯一 reducer 写入、Context 私有性、TEA purity 的可执行规则。

## 分阶段任务

### Task 1：六 Context 根与唯一 reducer Red/Green

1. 在 `model/root_tests.rs` 建立失败测试：TuiModel 对外只暴露不可变 Context accessor；生产模块不能取得 `&mut`。
2. 拆 `ConversationModel` 的 Config / Workspace 投影为独立 `ConfigProjection`、`WorkspaceProjection`；将 TuiModel 字段改为 private。
3. 引入 `AgentIntent`，每个 Context 独立变体；root reducer 穷尽 dispatch 并汇总 ModelChange / dirty。
4. 将 App 初始化、输入键盘、旧 event bridge 临时改为调用 reducer façade；禁止新增直接 `apply()`。
5. 添加 L0 静态测试：生产 `.apply()` 调用仅允许 root reducer；fixture/test 路径例外必须结构化。

### Task 2：RunProjection 与互补 Conversation 投影

1. 在 conversation model 定义 `RunProjection` / `RunStepProjection`、`RunProjectionStatus` / `RunStepProjectionStatus`。
2. 写失败的 L1 状态机测试：Created→Running、Running→AwaitingUser、AwaitingUser→Running、live→Cancelling→Cancelled、终态拒绝回退。
3. 实现 lifecycle ConversationIntent，确保 runs / timeline 重叠事实在同一 reducer 事务更新，revision 每事务一次。
4. 建立 L2 invariant 测试：run id、step id、tool reference、顺序和终态一致；timeline-only block 不伪造结构字段。
5. 不从 AgentClient outcome 直接改变 Run；只接受 Runtime lifecycle Intent。

### Task 3：InteractionState 与 body-specific Intent / Change

1. 定义 TUI-owned `InteractionState { request_id, run_id, body, draft, phase }`，四种 body 和 typed draft；同一时刻仅一个 active interaction。
2. 先写 L1：ShowInteraction、UpdateDraft、Confirm、Cancel、InvalidReply / CancelRejected 回退、duplicate request diagnostic conflict。
3. ShowInteraction 仅建立 interaction block，Run 是否进入 AwaitingUser 只由 Run lifecycle Intent 决定。
4. Interaction result intent 只改变匹配 request 的 phase；`InteractionReplySent` / `InteractionCancelled` 不改变 Run。
5. body-specific reply / typed cancel 仅由 Change 表达需求，不在 Model 持 sender 或调用 AgentClient。

### Task 4：Coordinator Effect 与 result Intent

1. 为 `InteractionReplyRequested`、`InteractionCancelRequested`、`RunCancellationRequested`、`WorkspaceMetadataRequested` 建立失败的 Change→Effect 测试。
2. Coordinator 将 Change 纯映射为 `SendInteractionReply`、`CancelInteraction`、`RequestRunCancellation`、`ResolveWorkspaceMetadata`；去重 render request。
3. EffectExecutor 仅在此调用 `AgentClient::reply_interaction` / `cancel_interaction`；穷尽 `InteractionCommandOutcome` 转为 AgentIntent。
4. 写 L2 测试，覆盖 invalid / not found / already completed / RunCancelling / irrecoverable failure，证明每种 outcome 的 result Intent。
5. 由 #943 接入 SDK event 后，补 L3 四类 body identity 和 L4 reply/cancel journey；本 PR 不伪造 SDK event。

### Task 5：Workspace revision 与 ViewState 边界

1. Workspace snapshot 只更新 `WorkspaceProjection { root, revision, stack }`，产生 metadata Effect。
2. metadata result 只有在 root + revision 均匹配时 apply；写 L1 stale result 测试和 L2 Effect 回灌测试。
3. 移除 ViewState 中业务 phase / run active 镜像；只保留 animation frame 等视觉事实。
4. 写 Guard 与故意违规探针：非 reducer model write、update I/O、ViewState import render、Context public mutable accessor 均 exit 2。

### Task 6：治理与验收

1. 更新 Migration Governance O6，逐项记录 #944 已对齐与 #943/#947/#878/#1246 的未闭合责任。
2. 回填 #944 的文档—代码差异、Guard 预算和 L0–L4 证据。
3. 依次执行定向 root / conversation / interaction / coordinator / workspace 测试、全 workspace 测试、fmt、clippy、architecture guards 与 diff check。
4. 请求独立 code review；发现 sender、旁路 mutation 或重复状态源时，在进入 PR 前修复或拆 Issue。

## 验收命令

1. `cargo test -p cli tui::model::root`
2. `cargo test -p cli tui::update::root_reducer`
3. `cargo test -p cli tui::model::conversation`
4. `cargo test -p cli tui::update::coordinator`
5. `cargo fmt --all -- --check`
6. `cargo clippy -p cli --all-targets -- -D warnings`
7. `PATH="/opt/homebrew/bin:$PATH" bash .agents/hooks/check-tui-tea-purity.sh`
8. `PATH="/opt/homebrew/bin:$PATH" bash .agents/hooks/check-tui-effect-boundary.sh`
9. `PATH="/opt/homebrew/bin:$PATH" bash .agents/hooks/check-tui-model-view-boundaries.sh`
10. `PATH="/opt/homebrew/bin:$PATH" bash .agents/hooks/check-architecture-guards.sh`
11. `cargo test --workspace`
12. `git diff --check`

## 拆分规则

- 若 RunProjection 需要 Runtime 生产事件字段，先等待 #943 / #878，禁止在 #944 自定义 SDK 平行 DTO。
- 若删除旧 `update_ui`、AskUser sender、同步 git 或 render 镜像，转交 #947；#944 仅提供可消费的纯接口。
- 若 CancelRun 需要实际 AgentClient 命令切换，转交 #945 / #878；本 Issue只定义 Change / Effect 结构。
- 任一 Guard 需要白名单例外时，停止并登记 #1021；本 Issue预算为零增长。
