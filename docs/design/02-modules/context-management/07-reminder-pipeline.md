# Context Management · Reminder 统一管线

> 层级：02-modules / context-management（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.2.0｜对应 Issue：[#1695](https://github.com/rushsinging/aemeath/issues/1695)
> 本文定义 reminder 的统一管线：来源注册（build）、Run 级队列（queue）、invocation 边界注入（inject）与广义策略模型。reminder 是 Context Window 的动态组成部分——运行期事实（任务进度、配置变化、后台任务完成、记忆更新）以受控方式进入模型上下文，对抗注意力衰减并承载事件回注。

## 1. 定位

Reminder 管线是 ContextPort `build_window` 的内部步骤之一：

```
build_window
  ├─ L2-L4 compact 读模型投影（L1 已在 ToolResult 入链前完成）
  ├─ await prompt 组装（PromptPipeline）
  ├─ memory 注入（MemoryPort）
  ├─ active summary
  ├─ reminder 注入（本管线）          ← 本文
  └─ → ContextWindow.system_blocks + messages
```

reminder 解决的问题域：

| 问题 | 手段 |
|---|---|
| 注入内容随窗口增长沉底（注意力衰减） | 稳定 step 间隔的周期性重注入 |
| compact 后注入内容随历史被 summary 替代而丢失 | per-kind compact 处置（重建 / 复位 / 丢弃） |
| 新增 reminder 类型需触碰 enum / 渲染 / 生成点多处 | 类型与机制解耦（source 注册，开闭原则） |
| 运行期事件（后台任务完成）需要回注模型 | 事件驱动入队与注入 |

**域归属决策**：管线归 Context Management，事实来源归 Agent Runtime。注入时机、placement、去重、预算、compact 处置是 prompt 组装知识；Run 生命周期信号与业务事件是 runtime 编排事实。Runtime 只推 typed 事件，不解释注入；Context 只按策略消费事件，不依赖 task store / 后台任务内部结构。依赖方向保持 `runtime → context`。

## 2. 三段固定管线

### 2.1 Build —— ReminderSource（开闭扩展点）

每个 reminder kind 一个 source。source 是唯一扩展点：新增 kind 只写一个 source 实现并注册，**NEVER** 触碰队列、注入调度、gate 或渲染分发代码。

```rust
struct ReminderPolicy {
    refresh: RefreshTrigger,   // 何时 build
    placement: Placement,      // 注入哪
    inject: InjectBehavior,    // 怎么注：dedup / priority / budget
    compact: CompactBehavior,  // compact 处置
}

trait ReminderSource: Send + Sync {
    fn kind(&self) -> ReminderKind;
    fn policy(&self) -> ReminderPolicy;
    fn build(&self) -> ReminderPayload;             // 读当前快照
    fn render(&self, payload: &ReminderPayload) -> RenderedBody;  // zh/en 双语
}
```

`ReminderRegistry` 收集注册的 source；数据源读取发生在 `build()` 内（task 快照、模型比较、事件缓冲），Context 不主动轮询业务 store。

### 2.2 Queue —— Run 级队列

Run-scoped 队列由 Runtime 在 Run 启动时创建句柄、Run 结束销毁（生命周期 owner 是 Runtime，状态与渲染 owner 是 Context）：

- entry = `{ kind, payload, fingerprint, enqueued_at, seq }`
- **入队**：refresh 触发时 push；同 kind 旧 entry 未消费时，快照类**替换**（只留最新），事件类**可累积**（各自独立 entry）
- **去重**：fingerprint（kind + 内容 hash）与最近已注入记录比对，相同不注入
- **compact 处置**：auto-compact `Committed` 后按 per-kind 策略分发——`Rebuild` 清空旧 entry 并标记下次注入前重建、`Reinstate` 保留 entry 原样重注入、`Drop` 丢弃且本 Run 不再注入；`Skipped` 不触发任何处置。原「一次性 bool gate」语义由此泛化
- **预算**：单次注入 reminder 总 token 封顶，超限按 priority 截断（截断块滞留回队，见 §2.3）

### 2.3 Inject —— invocation 边界注入

Runtime 每次 step 组装调用 `build_window` 时，管线按 policy 自动决策并注入：

- 注入即消费（drain）；`OnStepInterval` 类注入时**现场重建**取最新快照，**NEVER** 注入陈旧 payload
- 单 step 多 reminder 拼装（见 §5）
- 日志全链路：`created → enqueued → injected / skipped(dedup)`，按 kind 可关联（TargetCatalog 见 [logging](../logging/README.md)）

## 3. 广义策略：ReminderPolicy

四维一体声明，注入机制只解释策略、**NEVER** 按 kind 硬编码：

| 维度 | 取值 | 语义 |
|---|---|---|
| `refresh` | `OnRunStart` / `OnCompact` / `OnTaskMutation` / `OnStepInterval(n)` / `OnEvent(source)` | 何时 build：Run 启动 / compact 后 / task store 变更后下一请求 / 稳定 step 间隔 / 指定事件到达 |
| `placement` | `TailUserMessage` / `SystemTail` | 尾部 pending user message / system block 尾部 |
| `inject` | `{ dedup, priority, budget }` | dedup：`SkipIfUnchanged`（默认）/ `AlwaysInject`；priority：排序与截断序；budget：并入单次封顶 |
| `compact` | `Rebuild` / `Reinstate` / `Drop` | compact `Committed` 后的队列处置 |

priority 全序约定（仅作缺省，kind 可覆写声明）：**事件类 > 任务状态类 > 环境类**。

## 4. Placement 与缓存不变量

| 位置 | 适用 | 缓存影响 |
|---|---|---|
| `TailUserMessage`（默认） | Run 内会变的（周期重注入、事件驱动） | cacheable prefix 冻结，零影响 |
| `SystemTail` | Run 启动后恒定的（模型不匹配、guidance 变化） | Run 内等效稳定；跨 Run 本来就断缓存 |

**硬约束**：`refresh` 含 `OnStepInterval` / `OnEvent` 的 kind 强制 `TailUserMessage`。该约束由类型系统或架构守卫保证，**NEVER** 依赖约定。动态内容进入 system / cacheable prefix 视为违规。

## 5. 格式契约与单 step 拼装

### 5.1 统一 envelope

```
<system-reminder kind="task-progress" version="1" at="2026-10-04T01:00:00+08:00" seq="7">
  …body…
</system-reminder>
```

- envelope 字段（kind / version / at / seq）统一：日志关联、TUI 剥离、fingerprint 计算、compact 重建识别都依赖它
- body 由 source 的 `render()` 生成（声明式模板，zh/en 双语）；`version` 支撑格式演进与解析兼容

### 5.2 多 reminder 拼装

单次注入命中多个待注入 entry 时：

- **形态**：合并为**同一条**尾部 pending user message，内含多个 `<system-reminder>` 块；**NEVER** 拆成多条 user message（避免相邻 user-user 轮次破坏对话结构，前缀缓存影响最小）
- **排序**：块内顺序由 `inject.priority` 决定；**NEVER** 按 kind 枚举序硬编码
- **截断与滞留**：总 token 超预算时按 priority 从低到高截断；被截断块**回队滞留**（保 fingerprint），下一轮注入时最高优先补入，**NEVER** 静默丢弃——避免低优先级 reminder 饿死
- **同 kind 多 entry**：快照类注入前折叠为最新快照；事件类可保留多块（各自独立 envelope），由 source 的 inject 声明决定合并或并列

## 6. 事件源与 Run 生命周期

Runtime 推送 typed 事件（Context 定义事件 PL，Runtime 实现/转发）：

| 事件 | 触发点 | 消费 kind | 落地状态 |
|---|---|---|---|
| `RunStarted` | Run 启动（main / derived） | OnRunStart 类入队 + OnStepInterval 以 step=0 推进（首次注入） | ✅ |
| `StepAdvanced` | step 边界（accept_step_input） | OnStepInterval 计数 | ✅ |
| `CompactCommitted` | auto-compact 提交（Context `compact` 的 `Committed` 分支内部对接，不经 Runtime 推送——run_id 取自 CompactRequestData，少一次跨域往返） | 各 kind 的 compact 处置 | ✅ |
| `TaskMutated` | task store 变更后 | TaskProgress | 未接线：OnStepInterval 周期已覆盖 task 变更反映，即时触发留后续按需接入 |
| `BackgroundTaskCompleted` | 后台任务终态 | BackgroundTaskEvent（见 §8） | 随后台任务模型落地 |
| `MemoryUpdated` | memory 更新通知 | MemoryUpdated | 按 Run 启动事实 source 承载（见 §7 映射注记） |

事件 payload 为自包含快照数据；Context **NEVER** 回查业务 store，**NEVER** 持有 task / 后台任务内部句柄。

事件 payload 为自包含快照数据；Context **NEVER** 回查业务 store，**NEVER** 持有 task / 后台任务内部句柄。

## 7. 现有 reminder 迁移映射

| kind | 数据源 | refresh | compact | placement |
|---|---|---|---|---|
| TaskProgress | task 快照（计数 + 可见窗口） | `OnStepInterval(8)`（`run_started` 以 step=0 提供首次注入——0 是任意间隔的倍数，无需 OnRunStart 双声明） | Rebuild | TailUserMessage |
| GuidanceSourcesChanged | turn 边界 config diff（变更文件路径列表） | `OnRunStart` | Reinstate | SystemTail |
| ModelGuidanceMismatch | session 冻结模型 vs run 模型 | `OnRunStart` | Reinstate | SystemTail |
| MemoryUpdated | memory 更新通知（Run 边界一次性取走） | `OnRunStart`（reflection notice 为 Run 边界事实；`OnEvent(memory)` 留待 reflection 运行态演进接入） | Drop | TailUserMessage |

迁移后快路径行为等价：Run 启动 build → 首个 invocation 注入，与现状一致；差异只在 compact 后重开、周期重注入与去重按 policy 生效。

### 受众边界

reminder 管线只承载 **LLM 受众**（invocation-only、可重算快照）；用户受众（TUI 提示、状态栏）走 Runtime 事件流 / SDK 事件通道。双受众事实由触发点扇出两个独立产物（如 config 变化：LLM 收 GuidanceSourcesChanged reminder，用户收 `ConfigReloaded` 事件），**NEVER** 在 reminder source 上声明 audience。Stop Hook 反馈同理不并入：它是 canonical 落盘的一次性事件事实（resume 后仍须可见），与 reminder 的可重算快照生命周期相反（reminder 尾部注入类已改为显式落盘，但仍随 compact 清理、非一次性事件事实）。

### GuidanceReloadPolicy 的落地口径

`GuidanceConfig.reload_policy` 三变体中，`Remind`（默认）是 `specs/3.9-config-compat.md` §155 规定形态：guidance / instruction 文件变更时，下一 Run 注入**带路径的 Read 引导 reminder**（LLM 自行 Read 重新读取，NEVER 重建 cacheable system prompt）——已由 `GuidanceSourcesChanged { paths }` 载体落地。`Inject`（前置 diff head）与 3.7 冻结、3.9 NEVER 重建规则冲突，待 spec 裁决后废弃或另行设计；`Confirm`（InteractionPort 用户确认）挂后续 issue。未实现变体按 Remind 兜底渲染并 warn。

## 8. 与后台任务事件的对接

后台任务（tool call 统一后台任务模型，见对应 Runtime 设计）是本管线的第一个新类型消费者：

- kind `BackgroundTaskEvent`：refresh = `OnEvent(task_terminal)`、compact = `Rebuild`（从任务状态重建）、placement = `TailUserMessage`
- **有 active Run**：任务完成事实作为 reminder 注入当前 Run 的后续 step
- **无 active Run**：Runtime 走 Wakeup Run 回注（属 Runtime 编排，不经本管线注入，但 Wakeup Run 组装时经同一管线渲染任务状态快照）

本管线（source 注册 + 策略分发 + 队列）**MUST** 先于后台任务 reminder 部分落地，否则事件注入会被迫硬编码返工。

## 9. 落盘语义与不变量（#1848 修订）

尾部注入类（TailUserMessage）reminder **显式落盘**：注入轮的完整消息记入管线 `pending_persist`，step 收口时由 `append_and_persist` 在 Context 内部 flush 并前置到提交消息头部（canonical 顺序 user 输入 → reminder → assistant/tool）。收益：注入轮后的请求前缀逐 token 稳定——上轮 assistant 回复全部 cache 命中，断点仅在最新注入处（invocation-only 形态下每注入一次即损失上轮回复缓存）。SystemTail 类不落盘；重试路径 fingerprint 幂等早退。

1. 尾部注入类 reminder 显式落盘（统一 envelope、随 compact 清理、fingerprint 幂等）；SystemTail 类与 SDK/TUI 事件流零落盘零影响（渲染层 envelope 剥离保持）
2. reminder **NEVER** 进入 cacheable prefix；动态 kind 强制 `TailUserMessage`
3. 注入内容永远是当下快照或显式事件，**NEVER** 陈旧 payload
4. 注入机制只解释 `ReminderPolicy`，**NEVER** 按 kind 硬编码时机 / 位置 / 去重 / 处置
5. 多 reminder 拼装**NEVER** 产生相邻 user-user 轮次
6. 队列 entry 滞留**NEVER** 静默丢弃（截断必滞留、下轮优先）
7. 事件 payload 自包含，Context **NEVER** 回查业务 store

## 10. 关联

- 统一语言：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md) §3 Context Management
- Compact 家族与 `Committed` / `Skipped` 语义：[02-compact.md](02-compact.md)
- build_window 步骤顺序：[README.md](README.md) §5
- Runtime 事件管线：[../runtime/08-event-pipeline-and-published-language.md](../runtime/08-event-pipeline-and-published-language.md)
- 迁移治理：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)
