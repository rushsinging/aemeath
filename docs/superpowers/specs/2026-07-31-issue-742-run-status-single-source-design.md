# Issue #742 Run 状态单一真相设计

> 本规格落实 Agent Runtime typed Run 状态经 SDK 与 TUI ACL 进入状态镜像、再由 ViewState / ViewAssembler 派生活动展示的完整切换。稳定架构规则同步维护在 `docs/design/**`；本文只承载实施决策与范围。

## 1. 目标

- Runtime `RunStatus` 是唯一执行生命周期事实。
- SDK 使用封闭 `RunStatusView`，禁止字符串状态。
- TUI Model 使用 `RunStateSnapshot`，不保存独立 spinner 生命周期。
- TUI ViewState 使用 `RunActivityState` 保存 Main Run 的本地单调展示时间与动画。
- ViewAssembler 产出单帧 `RunActivityView`，Render 不解释 Runtime 状态。
- 删除 `ModelStreamWaiting` 全链路以及 `chat_active`、存储型业务 phase、running tool counter。

## 2. Published Language 与 ACL

`RunStatusView` 与 Runtime 当前 `RunStatus` 变体一一对应。Runtime adapter 使用穷举 match 转换；TUI 第一层 ACL 再穷举转换为 `TuiRunStatus`，禁止 `Debug`、`Display`、JSON 或字符串中转。

`ChatEvent::RunTransitioned` 继续携带 `run_id`、`parent_run_id` 和 typed status。Main Run 由 `parent_run_id.is_none()` 定义。ACL 为 transition 产生 `ObserveRunStatus` Intent；第二层不得忽略该事件。

迁移期 `Cancelling / Cancelled` 仍在 Runtime 当前枚举中，因此 Published Language 必须无损接纳。它们不成为 TUI 自有转换许可表；用户可见 terminal cause 继续来自 Runtime 权威 terminal 事件。后续兼容状态物理删除不属于本规格。

## 3. Model 状态镜像

`RunStateSnapshot` 仅保存：

- `run_id`
- `parent_run_id`
- `status: TuiRunStatus`

Model 维护按 Run identity 索引的 snapshots 与 `active_main_run_id`。规则：

- 重复 `(run_id, status)` 幂等；
- terminal snapshot 不被迟到非终态回滚；
- transition 可在 Started 丢失时建立 snapshot；
- Sub snapshot 不更新 `active_main_run_id`；
- terminal Main snapshot 仍可保留供诊断，但不派生活动展示。

Tool、Hook、compact 与消息内容只更新其各自现有事实，不再启动、停止或迁移 Run 状态。

## 4. 活动展示

`RunActivityState` 位于 ViewState，保存：

- Main Run identity；
- Main `InvokingModel` 静默起点；
- 动画 frame；
- 同一活动区间稳定的 verb。

它不保存 `RunStatus`、业务 phase、active bool 或可见性。时间由可注入单调时钟提供，测试不使用 sleep。

ViewAssembler 联合 Main snapshot、运行 detail 与 activity state 产出 `RunActivityView`：

- `DrainingInput`：Preparing input；
- `PreparingContext`：Preparing context；
- `Compacting`：Compacting，可附进度；
- `InvokingModel`：主活动行；
- `ApplyingResponse`：Applying response；
- `ExecutingTools`：Calling tool(s)，缺少 detail 时使用通用文案；
- `CancellingStep`：Cancelling step；
- `FinalizingStep`：Finalizing step；
- 兼容 `Cancelling`：兼容活动文案；
- `Terminating`：Terminating；
- `Created`、等待用户/审批状态及全部终态：不显示活动 spinner。

## 5. Main InvokingModel 静默占位

进入 Main `InvokingModel` 时开始计时。连续 10 秒没有有效可展示模型消息后，Output assembler 派生独立临时 block：

- 文案固定为 `Thinking.` / `Thinking..` / `Thinking...`；
- 使用 THINKING 语义色；
- 同一静默区间 block identity 稳定；
- 不写入 timeline、history 或持久化；
- 不显示阶段或等待秒数。

以下事件重置 Main 静默起点：

- 非空 Text delta；
- 非空 Thinking delta；
- ToolCallStart；
- 参数内容实际变化的 ToolCallUpdate。

Usage、重复状态、空 delta、日志、诊断、控制事件与 Sub Run 事件不重置。离开 `InvokingModel` 立即清除计时；再次进入重新开始。复用现有 Tick，不创建异步 timer 或 heartbeat。

## 6. 删除范围

本次删除：

- Runtime waiting task、`should_emit_model_stream_waiting` 与 RuntimeStreamEvent 变体；
- SDK `ChatEvent::ModelStreamWaiting`；
- TUI DTO / mapper / Intent / Model placeholder / assembler 分支；
- `chat_active`；
- Model 中存储型业务 `SpinnerPhase`；
- `running_tool_count` 生命周期；
- Start/Generate/Think/Pause/Resume/ForceIdle/SetSpinnerPhase/StopSpinner 等独立活动写入口；
- Hook/Tool/Compact/Turn 内容事件直接写活动生命周期的分支。

本次保留：

- Runtime 状态机本身；
- 权威 terminal 事件；
- Tool/Hook/compact detail；
- 与 Issue #742 无关的旧 UiEvent、cancel compatibility 和 Render 旁路，它们由后续退役工作处理。

## 7. 错误与异常语义

- 长期静默不表示失败，TUI 不自行生成 error。
- SDK transport 断开使用连接错误语义，不伪造 Run status。
- Provider timeout/retry/error 仍由 Runtime 负责。
- detail 缺失时活动文案降级，不改变生命周期。
- 乱序或陈旧状态不得回滚 terminal snapshot。

## 8. 测试策略

- L1：SDK 枚举序列化/shape、TUI status 值、snapshot 幂等与 terminal 保护、activity 10 秒边界。
- L2：Runtime mapper、TUI reducer、ViewState + snapshot、ViewAssembler + activity state。
- L3：Runtime `RunStatus` → SDK `RunStatusView` 全变体；SDK → TUI ACL 全变体与身份完整性。
- L4：Main 状态活动矩阵、Sub 隔离、四类消息重置、占位出现/消失/重现、取消与终止场景。
- L0：production build、all-target clippy、架构 Guard；禁止字符串 status、第二状态源和 `ModelStreamWaiting` 复活。
- L5：不需要；真实 PTY 不增加本能力验证价值。

所有跨层变化按相邻边界建立测试，先失败、再实现、再通过。时间测试使用 fake monotonic clock，禁止短 sleep。

## 9. 文档同步

稳定规则同步更新：

- `docs/design/01-system/02-ubiquitous-language.md`
- `docs/design/02-modules/tui/01-architecture-and-dataflow.md`
- `docs/design/02-modules/tui/02-model.md`
- `docs/design/02-modules/tui/03-event-flow-and-acl.md`
- `docs/design/02-modules/tui/04-view-layer.md`
- `docs/design/03-engineering/03-migration-governance.md`

实现完成后回填最终代码路径、验证证据与延期责任。
