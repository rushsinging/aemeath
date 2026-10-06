# Agent Runtime · 后台任务系统（tool call 统一后台任务模型）

> 层级：02-modules / runtime（模块战术设计）
> 状态：Target（设计定稿，待实施）｜Milestone：v0.2.0｜对应 Issue：#252
> 本文定义 tool call 的统一后台任务模型：超阈值自动转后台、占位结果 + 异步回注、完成主动提醒（reminder / Wakeup Run）与后台任务查询。依赖 #1440（tool call identity / 硬超时 / 取消协议 / pending receipt）与 #1695（Reminder 统一管线）先行落地。

## 0. 决策记录

| # | 决策 |
|---|---|
| D1 | 不做独立 watch tool。所有 tool call 本质统一为后台任务，前台等待只是快路径视图 |
| D2 | 转后台语义 = 占位 tool result + 异步回注。全部工具 + 时间阈值自动触发，无 per-tool 声明；阈值内完成走快路径，行为与现状完全一致 |
| D3 | 完成提醒双路：有 active Run → 完成事实作为 reminder 注入当前 Run 后续 step；无 active Run → Runtime 创建 Wakeup Run 回注。agent 发起转后台即视为该任务的唤醒授权 |
| D4 | 任务生命周期随 CLI 进程终止，标记失效，不做 daemon 化 |
| D5 | 不挂 Goal / Loop。tool call 后台任务是独立轻量任务记录，与 Workflow 的 `continuation_authorization` 是独立通道，不得混同 |
| D6 | 阈值可配置，默认 10s，随 `RunConfigSnapshot` 冻结（Run scope） |
| D7 | 设计文档随核心引擎 PR 落地（含 workflow 设计文档落地与修订） |
| D8 | 交付拆分为 3 个 PR：核心引擎+文档 → 通知链路 → 查询+持久化+TUI+收尾 |
| D9 | sequential FIFO 修订：前序调用转后台后，同轮后续 sequential-only 调用 MAY 启动（启动顺序仍严格 FIFO）；`ToolCallState` 增加 `Backgrounded` 中间态 |
| D10 | 停止通道：语义上仅经 `background_tasks` tool 的 stop action；TUI 提供后台任务查看/管理 slash 命令；terminate（退出）时后台任务 drain 退出 |
| D11 | Wakeup Run 触发事实走 reminder 落盘（TailUserMessage 显式落盘机制，与 #1848 同构）+ SDK 事件渲染 TUI 卡片；不合成用户 turn |
| D12 | `logs` 查询含增量游标 + 非消耗性读取，运行中与完成后皆可查 |
| D13 | wakeup 到来直接启动 Wakeup Run（用户可 Esc 标准取消），竞争仲裁暂不实现 |

## 1. 目标 / 非目标

**目标**：

- 任意 tool call 超过阈值自动转后台；agent 收到占位结果并可继续其他工作或结束 turn。
- 后台任务完成后主动提醒 LLM：有 active Run 时以 reminder 注入当前 Run 后续 step；无 active Run 时经 Wakeup Run 回注。
- agent 可通过查询 tool 查看后台任务状态与日志（含增量游标），可停止任务并获得明确终态。
- CLI 退出 / 重启后，后台任务标记失效且可从 session 恢复中看到。
- 快路径行为与现状一致（阈值内完成无行为变化）。

**非目标**：

- daemon 化跨进程任务；
- 独立 watch tool 或 per-tool 后台声明；
- Workflow / Goal / Loop 编排接入（仅共享唤醒机制与文档不变量对齐）。

## 2. 核心抽象

### 2.1 双状态机分工

现有 `ToolCallReceiptData{Pending, Running, Terminal}` 管理逐 call 的**执行事实**（取消协议、重启恢复）；新增 `BackgroundTaskRecord` 管理跨 Run 的**任务监督**（回注、查询）。两者以 `ToolCallIdentityData`（session / run / step / call 四级 identity）关联，各自单一职责：

```text
BackgroundTaskRecord
  ├─ task_id: TaskId（UUIDv7）
  ├─ identity: ToolCallIdentityData
  ├─ tool_name / invocation_summary（参数摘要，审计与查询展示）
  ├─ state:
  │     Dispatched → ForegroundWaiting ──阈值内完成──→ Terminal(Success | Failure | Cancelled | TimedOut | CancellationUnconfirmed)
  │                     └─超阈值→ Backgrounded → Completed | Failed | TimedOut | Stopped | Invalidated
  ├─ backgrounded_at / deadline_snapshot（转后台时三重 deadline 最早值快照）
  ├─ output: OutputRingBuffer（字节上限 + 读写游标）
  └─ terminal_output: Option<ToolExecutionOutcome>（终态完整结果）
```

**receipt 状态机扩展**：`ToolCallState` 增加 `Backgrounded` 中间态（`advance` 单调规则同步扩展）。转后台时 receipt `Running → Backgrounded`，**terminal 仍推迟到真实完成时写入**——#1440 终态语义不变，只推迟时点：

- step finalize 允许携带 `Backgrounded` receipt 挂起（区别于取消场景的 unfinished 收敛）；
- 重启恢复时 `Backgrounded` receipt 与后台任务账本对账后投影为「任务已失效」（见 §6）。

### 2.2 BackgroundTaskSupervisor（session 级监督器）

- **Session 级存活**（非 Run 级），Run 收口不销毁；CLI 退出时统一 drain（见 §7）。
- 职责：派发登记、阈值判定、后台 `JoinHandle` 托管、输出收集、终态推进、通知路由（§4）、持久化（§6）。
- 摆放：runtime feature `application/tool/` 旁新模块；**NEVER** 流入 tools domain（工具执行编排边界不变）。

### 2.3 输出管理与持久化（任务日志文件：直绑优先 + 终态兜底）

`ToolExecutionOutcome` 为一次性返回，无内置增量输出流。完整输出零丢失
（2026-10-07 拍板，方案 B；同日修订为输出直绑形态）：

- **子进程类工具（Bash 优先）直绑任务日志文件**：派发即创建 per-task 日志文件
  （append 模式），路径经 `ToolExecutionContext` 注入；工具把子进程
  stdout/stderr 直接重定向到该 fd——零跳转、字节保真、无捕获上限、
  子进程退出由 OS 关闭 fd（规避 runtime 侧延迟 close 的丢记录窗口）。
  工具层以 opt-in 能力声明参与（descriptor 声明输出直绑），**NEVER** 强制
  全部工具感知日志文件。
- **非流式工具**：无中间输出；runtime 在终态把 `ToolExecutionOutcome` 结果
  append 进同一文件（唯一写入方）。
- **progress 通道职责分离**：保留用于 TUI 实时滚动直播，**不再作为日志文件的
  数据源**——文件是持久真相，progress 是 UI 即时视图。
- 内存 `OutputRingBuffer` 退化为可选优化（文件 tail 读取代）；`logs` 查询
  直接按文件区间读取（LLM 增量游标与 TUI 翻全量共用同一文件真相源），
  token budget 截断在读取层叠加。
- 生命周期随 session：会话任务日志 GC 与 resume 失效对账同步（§6）。

### 2.4 阈值配置

- 配置项 `runtime.tool_background_threshold_secs`，默认 10；env `AEMEATH_TOOL_BACKGROUND_THRESHOLD_SECS` 覆盖；随 `RunConfigSnapshot` 冻结（Run scope）。
- `0` = 禁用后台化（纯快路径）。

> trade-off：10s 意味着 10s–120s 的中等命令从「本轮等到结果」变为「占位 + 稍后通知」。模型视角结果可用时点后移、turn 吞吐提升；若实践发现通知往返成本高于等待收益，可上调默认值（一处配置）。

## 3. 执行流改造

### 3.1 spawn-first 统一执行模型

`ToolExecutionSupervisor` 从「直接 await future」改为：每个 call 先 `tokio::spawn`（owned `ToolExecutionContext` move 进 async block；`ToolExecutionPort::execute` 为 async_trait boxed future + `Arc<dyn ToolExecutionPort>`，形态支持），前台等待变为四路 select：

```text
select! {
  result = join_handle        → 快路径：与现状完全一致（含 grace / cancel / terminal receipt 现逻辑）
  _      = sleep(阈值)         → 转后台：不 abort，登记 TaskSupervisor，前台返回占位结果
  _      = hard_deadline       → 硬超时（现状三重 deadline 语义不变）
  cancel = cancellation_signal → 取消协议（现状语义不变）
}
```

**快路径不变性以测试锁定**：阈值内完成的消息流、receipt 时序、事件与现状逐项等价（L4 对比测试）。

### 3.2 sequential FIFO 语义修订（D9）

背景：Bash 为 `is_concurrency_safe=false`（sequential-only）且 timeout 3600s——后台化的主场景恰在 sequential 路径。现行约束「前序调用完成执行、cleanup、terminal receipt 与 ToolResult 发布后才可启动后序」会使同轮后续调用在转后台后仍被卡住，后台化落空。

**修订**：前序调用转后台后，其**前台契约**视为已履行——占位 ToolResult 已发布、receipt 进入 `Backgrounded`、cleanup 延期至终态——同轮后续 sequential-only 调用 MAY 启动，**启动顺序仍严格 FIFO**（提交顺序不变）。specs 3.4.3 / 3.4.4 同步修订。

风险与缓解：乱序风险窗口 = 「同轮预提交 + 真实顺序依赖 + 前序恰好超阈值」。实践中模型对有依赖的命令通常分轮发（看到结果再发下一个）；分轮场景模型已看到占位结果、知情决策；占位文案明确告知「该命令仍在后台、结果未定」。

### 3.3 流式路径（边流边执行）

`StreamingToolExecutor` 已 spawn 化，generation / step 收口 `detach_invocation` = cancel + 取回 handles。改造：detach 点判定任务是否超阈值未完成——是则转后台登记而非 cancel。

**分阶段**：核心引擎 PR 覆盖 ordinary 路径（流式保持现状、零退化）；收尾 PR 接入流式（行为完整性要求，非可选增强，交付说明中披露）。

### 3.4 硬超时延续

转后台时将当时三重 deadline（tool timeout / 剩余 run time / step 预算）最早值**拍快照**存入 task record，由监督器在后台继续生效；到点走 #1440 收敛（cancel + grace + `CancellationUnconfirmed` 不确定语义不变）。Run 提前收口不影响已快照的 deadline。

## 4. 完成通知（两级送达）

### 4.1 占位 tool result

- 转后台时经现有 `finalize_tool_round_results` 物化链合成单条 `Message::tool_results_rich`（`tool_use_id` 配对完整，`is_error=false`），与取消收敛合成（`converge_cancelled_tool_round`）同构，不触发 `message_integrity` 孤儿清理。
- 文案双语：任务 id +「已转后台运行，完成后将收到通知；可用 `background_tasks` 工具查询状态 / 日志或停止」。

### 4.2 送达分层

| 层 | 载体 | 内容 |
|---|---|---|
| 轻量通知 | reminder（`BackgroundTaskEvent`） | task_id、工具名、终态、输出尾部截断（注入 token 预算内） |
| 完整数据 | `background_tasks` tool | ring buffer 增量读取 + token budget 截断 |

reminder 不携带完整输出（管线预算纪律）。

### 4.3 有 active Run

任务终态 → 监督器调 `ContextPort::reminder_handle_event(run_id, "background_task")` → 管线 `push_event` → 下一次 invocation 注入。新增 source：`OnEvent + TailUserMessage + SkipIfUnchanged(event) + Rebuild`（注册期 `is_valid` 校验已拒绝动态 refresh 配 SystemTail）。`AwaitingUser` 相位不注入（管线天然在下一次 build_window 生效，符合「不打断已发出的 Model Invocation」）。active 判定：`ActiveRunRegistry.current_main_run_id`。

### 4.4 无 active Run：Wakeup Run（D11 / D13）

- `RunIntent` 新增变体 `BackgroundTaskWakeup { task_ids: Vec<TaskId> }`。
- `WakeupMailbox`（Runtime 内部 mpsc，session 级）：任务终态时若无 active run → send。
- session driver idle 等待 select { 用户输入, wakeup }；收到 wakeup → 以 `BackgroundTaskWakeup` intent 启动 Main Run（直接启动，用户可 Esc 走标准取消协议）。
- 触发事实由 `BackgroundTaskEvent` reminder 承载：TailUserMessage 类 reminder 随 step 收口**显式落盘 canonical**（envelope 标识、compact 可清理、resume 可见），与既有尾部注入机制同构；**不合成用户 turn**。Run 的 `user_input` 为最小内部触发标记（空输入可行性在通知链路 PR 实现时验证，不行则用极简标记文本）。
- 用户侧显示走 SDK `BackgroundTask` 生命周期事件渲染卡片（见 §8），不依赖 canonical 消息样式。

## 5. 后台任务查询 tool

单一 tool `background_tasks`，action 枚举参数：

| action | 行为 |
|---|---|
| `list` | 活动与近期任务（id / 工具 / 状态 / 时长） |
| `status` | 单任务详情：状态、终态、deadline 剩余 |
| `logs` | 查询任务日志（运行中与完成后皆可）：ring buffer 非消耗性读取；`tail` 参数指定尾部行数（默认 50）；返回读取游标，下次携带游标只读新增（增量游标）；token budget 截断；多次读取幂等、不破坏后续回注 |
| `stop` | 请求取消：signal cancel + grace + terminal receipt，返回明确终态（含 `CancellationUnconfirmed` 不确定语义） |

挂 Main + Sub Catalog，常规 profile 权限链；占位 tool_result 文案中显式引导 `logs` 用法。

## 6. 持久化与 resume

- 新 storage namespace `BackgroundTask`（AtomicBlob，ProcessCrashSafe），key `background-tasks-{session_id}`；存 task record 与任务日志文件引用。
- 任务日志文件（§2.3 方案 B）：per-task 增量 append，完整输出真相源；会话任务日志随 session GC。
- resume：`Backgrounded` / `Running` → `Invalidated(reason: process_exit)`；查询 tool 可见失效任务。
- **对账**：receipt 恢复路径（`has_unfinished_receipts` → unconfirmed 投影）遇 `Backgrounded` receipt 时与任务账本对账——有对应 task record 则投影为「任务已失效」而非 unconfirmed。specs 3.10 同步。

## 7. 取消 / 审计 / 退出集成

- **CancelStep**：`ForegroundWaiting` 期 = 现状取消协议；`Backgrounded` 后不受 CancelStep 管辖（step 已收口），停止仅经 `background_tasks` stop（D10）。
- **terminate / CLI 退出**：监督器 drain——全部后台任务 cancel + wait grace + 标 `Invalidated` + 落盘（D10）。
- **权限**：派发时已评估，转后台不重新评估；`stop` 走正常 tool 权限链。
- **审计**：任务终态事实与现有 tool 终态审计同构。
- **Usage**：后台任务无 model invocation，不构造 `UsageRecord`。

## 8. TUI / SDK 与 LLM 引导

- 转后台瞬间：工具块追加「已转后台（task_id）」状态行。
- TUI 新增后台任务查看 / 管理 slash 命令（D10；经 Tools-owned Command Catalog，Runtime PendingCommand 为 handler adapter）；可翻阅任务日志文件全量输出（§2.3）。
- **LLM 行为引导（2026-10-07 拍板）**：除占位 tool_result 文案外，Main/Sub 的 system
  prompt 经 guidance 系统注入后台任务特性说明——统一模型（所有 tool call 超阈值自动
  转后台）、占位结果语义（非终态，完成会主动通知）、`background_tasks` 查询/停止用法、
  sequential 顺序提示（前序转后台后同轮后续命令可能与未完成前序并行，有顺序依赖时
  应等待通知或先查状态）。随查询 tool（PR3）落地时一并接线 prompt BC。
- SDK 新增 `BackgroundTask` 生命周期事件（PL）→ TUI reducer：
  - 消息流系统样式卡片（可折叠）：「⏙ 后台任务完成：task-xxx『cargo test』已完成，已唤醒 agent 继续」——用户清楚看到 agent 为何自己动起来；
  - 后台任务面板：活动任务状态 / 输出预览（复用 Bash 输出渲染），管理入口。

### 8.1 自动转后台覆盖面

普通工具与 Agent（sub-agent 派发）共用同一 `ToolExecutionPort` 执行路径，**天然全部
自动转后台**（Agent timeout 3600s，是最需要后台化的场景）；交互类（AskUser）的 tool
future 以 `Suspended` 即时返回（等待由 Runtime interaction waiter 承担），不会触发
阈值，天然安全。流式路径（#1494 边流边执行）的超阈值转后台在收尾 PR 接入
（此前保持 detach-cancel 现状，零退化）。

## 9. 与 Workflow 设计的对接修订

`01-workflow-design.md`（未落地，随核心引擎 PR 一并落地）需修订：

1. §9.10 / §11：`invocation_reminders` 旧机制名 → Reminder 统一管线术语（ReminderSource / policy / envelope）。
2. 不变量 17 / 18 对齐 D3：无 active Run 时由 Wakeup Run 回注；授权来源 = agent 发起转后台。
3. 新增不变量：tool call 后台化唤醒与 `continuation_authorization` 是独立通道，前者授权来自 agent 发起转后台动作本身，后者管辖 Goal / Loop 级长任务后台继续，两者不得混同。
4. `07-reminder-pipeline.md` §6 / §7 / §8：`BackgroundTaskEvent` 从「预留」改「已接线」。

specs 同步（核心引擎 PR 内）：3.4.3（FIFO 修订）、3.4.4（`Backgrounded` receipt / deadline 快照）、3.9（阈值配置项）、3.10（新 namespace / 对账）。

## 10. 交付拆分（D8）

| PR | 内容 | 依赖 |
|---|---|---|
| 核心引擎 + 文档 | 领域模型、receipt `Backgrounded` 态、监督器、spawn-first 执行流、占位协议、快路径不变性、阈值配置、§9 全部文档 | — |
| 通知链路 | reminder 接线（source + 触发点）、`WakeupMailbox`、`RunIntent` 变体、Wakeup Run | PR1 |
| 查询 + 持久化 + TUI + 收尾 | 查询 tool（含增量游标）、task store、resume 对账、TUI / SDK 事件、slash 命令、流式路径后台化、审计、Guard / 退役检查 | PR1 |

## 11. 测试策略（L0–L5）

- **L0**：守卫——监督器 / 任务模型不进 tools domain；reminder kind 扩展不触碰管线分发代码。
- **L1**：task record 状态机单调性；阈值判定纯函数；占位文案合成；ring buffer 截断 / 幂等读 / 游标推进。
- **L2**：假 tool sleep > 阈值 → 占位 + 登记 + 后续 sequential 启动；OnEvent 注入；wakeup 触发；receipt `Backgrounded` 推进。
- **L3**：task store 持久化契约；resume 对账；SDK 事件 PL 契约。
- **L4**（核心）：长命令转后台 → 继续推理 → 完成通知 → 查询（含游标）；无 active run → Wakeup Run；resume 失效展示；**快路径等价**（阈值内全链路与现状消息流一致）；Main / Sub 双侧覆盖。
- **L5**：不新增 PTY smoke。

## 12. 风险与开放问题

| # | 项 | 处置 |
|---|---|---|
| ① | wakeup 与用户输入在 idle 边界竞争 | 直接开 Run（可 Esc）；实践扰度高再加「用户输入优先」仲裁（D13） |
| ② | 10s 默认值对中等命令节奏的影响 | §2.4 trade-off 已述，可一处配置调整 |
| ③ | `ToolExecutionContext` owned move 形态（Clone 或调用重构） | 核心 PR 实现时定，边界守卫不变 |
| ④ | progress 通道增量输出覆盖面 | 仅影响「运行中实时日志」体验，终态输出不受影响 |
| ⑤ | sequential FIFO 修订的完成序风险 | specs 修订 + L4 快路径 / 转后台双场景锁定 |
| ⑥ | Wakeup Run 空输入可行性 | 通知链路 PR 实现时验证，兜底极简标记文本 |
