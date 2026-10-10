# Memory · Reflection 引擎

> 层级：02-modules / memory（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0
> 本文定义 Reflection 引擎的领域模型、触发条件、prompt 构建、output schema、Memory-owned workflow，以及与 Runtime 的职责边界。§1/§4/§8 中关于 Run 归属与 `Reflecting` 状态、Activity 可见性、Manual 显式入口、Usage 记账与取消/超时的描述以**当前实现**为准；其余为目标态设计。

## 1. 定位

Reflection 是 Memory BC 内部的**领域服务**——它不调 LLM，不依赖 ProviderPort。它负责：

1. **构建 prompt**：把当前项目记忆 + 最近对话摘要组装为反思 prompt（纯函数，i18n）。
2. **解析 output**：把 LLM 返回的 JSON 解析为 `ReflectionOutput`（含 MemorySuggestion）。
3. **应用结果**：把 suggestion 转为 MemoryEntry，并通过当前 Run 的同一 `MemoryPort` 写入/合并、归档候选、标记过期记忆。
4. **历史事实**：定义 `ReflectionRecord` 与只读 `ReflectionHistoryQuery`；Runtime 执行通道完成后写入历史，`/reflect` 只查询这些记录。

Runtime 负责：
- **触发判断与 Run 归属**：interval / manual / pre-compact 三种来源的判定；三者统一进入 engine 的反思阶段，由 Runtime loop 状态机完成 `RunStatus::Reflecting` 的进入与返回转换——Interval/PreCompact 在 Conversation Run 内相位往返，Manual 创建 `RunIntent::ManualReflection` 的独立 Run。
- **状态与 Activity 发布**：`Reflecting` 状态转移、Reflection activity 的发布与终态收口由 Runtime loop 状态机（engine 反思阶段）唯一负责；Activity 是结构化观测事实，不是业务状态机。
- **LLM 调用**：经 ProviderPort 发起独立 LLM 调用，传入 Memory BC 构建的 prompt。
- **历史提交**：将完成结果交给 Memory-owned history adapter；完成后只发布不含正文的计数事实。

### 职责边界

| 职责 | 归属 | 说明 |
|---|---|---|
| prompt 模板构建 | Memory BC | 纯函数，知道记忆格式与反思需求 |
| output schema / parsing | Memory BC | MemorySuggestion 类型归 Memory |
| apply 逻辑（写入 / 标记过期）| Memory BC | 经 MemoryPort 操作 |
| 触发时机判定 | Runtime | interval / manual / pre-compact |
| `Reflecting` 状态转换与收口 | Runtime loop 状态机 | `BeginReflection` 进入、`ReflectionCompleted` 返回进入前状态；`Reflecting` 非终态 |
| Reflection activity 发布与终态收口 | Runtime loop 状态机 | engine 反思阶段唯一负责；观测失败只记 warn，不阻断反思 |
| LLM 调用 | Runtime | 经 ProviderPort |
| Reflection 配置消费 | 双方 | Config 下发 MemoryConfig，Runtime 读触发条件，Memory 读 apply 策略 |

Memory BC **不依赖** ProviderPort——这保持 Context Map 一致（Memory 无 Memory→Provider 边）。

## 2. MemorySuggestion

```rust
struct MemorySuggestion {                // Reflection 产出的候选记忆（VO）
    layer: MemoryLayer,                  // 建议写入的层（默认 Project）
    category: MemoryCategory,            // 建议的分类
    content: String,                     // 建议内容
    tags: Vec<String>,                   // 建议标签
    reason: String,                      // LLM 给出的建议理由
}
```

MemorySuggestion 是 **Reflection 的产出物**，不是 MemoryEntry——它还没有 id、created_at、last_confirmed_at。apply 时转换为 MemoryEntry（生成 UUIDv7 + 填充时间戳 + source = Llm）写入。

## 3. ReflectionOutput

```rust
struct ReflectionOutput {                // LLM 返回的完整反思结果
    deviations: Vec<String>,             // 偏差检测：对话中的偏离行为
    suggested_memories: Vec<MemorySuggestion>, // 建议新增的记忆
    outdated_memories: Vec<String>,      // 建议标记过期的记忆引用（解析后为 UUID 字符串）
}
```

- **deviations**：LLM 观察到 agent 在对话中有偏离预期行为的描述（如重复尝试失败方案、忽略用户指令）。正文只保存在 Memory-owned history record 中；TUI 的只读查询只取得计数等安全摘要。
- **suggested_memories**：LLM 认为值得持久化的新记忆建议。
- **outdated_memories**：LLM 认为已过时的已有记忆引用。模型侧只输出本次输入列表的行序号（如 `M3`）；解析阶段经引用表映射为真实 UUID 后进 apply（见「引用契约」小节）。字段保持 `Vec<String>`，兼容历史记录中未解析的原始 token。

### 反序列化兼容

LLM 可能返回 `null` 而非空数组。使用 `null_as_empty_vec` 自定义反序列化器处理：

```rust
#[serde(default, deserialize_with = "null_as_empty_vec")]
pub deviations: Vec<String>,
```

### 引用契约：模型只输出行序号

反思输入的记忆列表每行以本次运行内稳定的行序号开头（`- [M1] [Decision][tags] content`）。
模型对已有记忆的引用（`outdated_memories` / `supersedes` / `synthesizes`）**MUST** 使用行
序号（如 `M3`），**MUST NOT** 使用 UUID、标签或正文片段：UUID 是系统内部寻址标识，对
模型无语义，要求模型复刻长随机串必然出错。

- **引用表**：输入构造返回 `ReflectionReferenceTable`（行序号 → MemoryId）；行序与渲染
  顺序由同一 `entries` 切片推导（序号单一来源），模型输出里的序号经该表映射回真实 id。
  序号仅在产生它的那次反思运行内有效。
- **解析容错**：`parse_output` 逐条解析引用；无法解析的 token **MUST** 跳过并记录
  （`memory_reflection_reference_unresolved`），**NEVER** 让单条坏引用作废整批建议。
  合法 UUID 直填（兼容路径）仍被接受。
- **apply 容错**：`apply_reflection` 对 `outdated_memories` 中无法解析的条目同样跳过并
  记录（`memory_reflection_reference_skipped`）；`suggested_memories` 照常写入，NEVER
  因坏引用丢弃合法建议。

### 悬挂 Running 收口

`append_running` 在进程被终止（崩溃/重启）时可能留下永不收口的 `Running` 记录。Runtime
在每次反思执行前调用 `ReflectionWorkflow::reap_stale_running`（阈值 = 任务超时 × 2，
至少 60s），把超龄 `Running` 以同一 stable id upsert 为 `Failed(Interrupted)`；未超龄的
记录（并发中正在运行）不受影响；收口失败只告警、不阻断本次反思。

## 4. 触发条件

Runtime 负责判定是否触发 Reflection，Memory BC 提供配置读取辅助。当前实现中 Interval、PreCompact、Manual 三触发统一进入 engine 的反思阶段：`RunTransition::BeginReflection`、Reflection activity 发布与终态收口都在该阶段完成，调用方 await 到终态。

### 触发来源

```rust
enum ReflectionTrigger {
    Interval,       // 每 N 轮
    PreCompact,     // compact 前快照
    Manual,         // Runtime 显式手动请求；不是 /reflect 查询命令
}
```

Runtime 对三种来源统一做 enable 判定（等价 `reflection_enabled`：memory 总开关、`reflection.enabled` 与 `interval_runs > 0`）并构造拥有消息快照的请求。`Manual` 与 `PreCompact` 在启用 Reflection 时不受 interval 限制；三者共用同一执行通道，差别只在 Run 归属与状态路径。

### 三种触发时机

| 时机 | Trigger | Run 归属与状态路径 | 触发者 | 说明 |
|---|---|---|---|---|
| **轮次间隔** | `Interval` | Conversation Run 内 `ApplyingResponse → Reflecting → ApplyingResponse` | engine step driver | 每 `interval_runs`（默认 10，旧键 `interval_run_steps` 仍可读取）个 Main Run 命中一次；`run_count` per-session 计数，`/clear` 与 resume 切换后从 0 重数；**只在主 Run 的 `ModelStep::Complete` 收尾路径判定**（该跳无 tool call）；轮末等待反思完成 |
| **Pre-compact** | `PreCompact` | Conversation Run 内 `Compacting → Reflecting → Compacting` | engine 的 pre-compact 插入点 | compact 前冻结“将被丢弃”的 messages 快照；只有 `CompactOutcome::Committed` 后才在 `Compacting` 内进入 `Reflecting`，压缩收口在反思返回后放行；compact 失败、被 hook block、消息不足或取消时不执行 |
| **手动请求** | `Manual` | 独立 `RunIntent::ManualReflection` 的真实 Run：`DrainingInput → Reflecting → DrainingInput` | `/reflect-now` 命令 | idle 受理后创建真实 Run（Run root activity `purpose = Reflection`）；与另两种 trigger 共用同一执行通道；`/reflect [limit]` **NEVER** 进入此入口 |

### 反思游标（消息起止点，#1827）

反思消息快照的起止点由**游标**（`ReflectionRecord.coverage_end`，session active 历史的消息计数）驱动：

- **Interval**：Main Run 启动时读游标切片 session 历史增量（`IntervalReflectionMaterialSlot` 装槽），触发时与当前 Run 内消息拼接——覆盖上次反思以来的**所有中间 turn**（修复「只看当前 Run」的覆盖缺口）。Succeeded 后游标推进到「装槽时历史总长 + Run 内消息数」。
- **Manual**：`/reflect-now` 受理时读游标切片增量（修复全量历史无界的 token 浪费）；**增量为空（上次反思后无新对话）时跳过执行并提示**，NEVER 为空跑支付 token。Succeeded 后游标推进到快照时历史总长。
- **PreCompact**：保持「被丢弃段」抢救语义，**不读也不推进游标**（被丢弃段不是 session 历史切片）。
- **游标失效回退**：游标缺失（首次/旧记录）或失效（`cursor > 当前历史长度`，如 compact 截断后位置重排）→ Interval 退化为仅当前 Run 消息（不推进游标）、Manual 回退全量历史。
- **失败/取消不推进**：只有 `Succeeded` 终态把 `coverage_end` 写入记录；Failed/Cancelled/TimedOut 落盘时游标字段为空。
- **消息摘要预算**：`recent_messages_summary` 按字符预算（24,000）截断取最近部分——PreCompact 被丢弃段与 Manual 回退全量的防爆闸，NEVER 无界进入 prompt。

### Run 状态：`Reflecting`（非终态）

`RunStatus::Reflecting` 是可恢复的工作相位，**NEVER** 是 Run 终态（`is_terminal()` 只含 `Completed` / `Failed` / `Terminated`）。三条路径都经状态矩阵的 `RunTransition::BeginReflection` 进入、`RunTransition::ReflectionCompleted` 返回进入前状态（`reflection_return_status`）：

| Run 形态 | Run intent | 进入入口（状态 × intent gate） | 返回 |
|---|---|---|---|
| Conversation（Interval） | `RunIntent::Conversation` | `ApplyingResponse` | 回到 `ApplyingResponse`，继续回合收尾 |
| Conversation（PreCompact） | `RunIntent::Conversation` | `Compacting` | 回到 `Compacting`，再放行压缩收口 |
| Manual（`/reflect-now`） | `RunIntent::ManualReflection` | `DrainingInput`（`Run::begin_manual_reflection` 命令式入口，仍走状态矩阵） | 回到 `DrainingInput`，reason `ManualReflectionSettled`；由后续 drain 的 `DrainEmptyAndSealed` 收口 `Completed` |

- 其他「状态 × intent」组合一律拒绝（warn 日志 + `IllegalTransition`）：会话 Run 只能从 `ApplyingResponse` / `Compacting` 切入，手动反思 Run 只能从 `DrainingInput` 切入。
- **Run intent → Run root activity purpose 映射**：`Conversation` / `ManualCompaction` → `Main`，`ManualReflection` → `Reflection`。
- Manual Reflection Run **NEVER** 进入模型调用（`ContextPrepared` 对该 intent 拒绝）、**NEVER** 创建 RunStep、**NEVER** 走 `ContextPort::build_window`。

### Manual 显式入口链路

用户唯一可见入口是 slash 命令 `/reflect-now`（无参数）：

```text
TUI "/reflect-now"
  → SDK ChatInputEvent::ReflectNow
  → Runtime input gate:
      idle → PendingCommand::ReflectNow（只置受理标志，不裸 await 反思）
      busy → 入队 pending buffer 仅作排队回显；Run 收尾时统一丢弃+提示（见下）
  → 会话驱动 idle 分支:
      未启用（memory/reflection 关或 interval_runs == 0）
        → 直接 CommandResultText（DisabledSkipped 文案）；不创建 Run、不产生 activity
      启用 → bind_main_run 冻结 committed session 的 structured_messages() 快照
           → RunSpec::manual_reflection()（intent = ManualReflection）创建真实 Run
  → engine 手动反思阶段（主循环之前）:
      begin_manual_reflection：DrainingInput → Reflecting（root purpose = Reflection）
      → Reflection activity(Running, trigger = Manual)
      → ManualReflectionPort 执行一次反思（无 RunStep、不调用主模型）
      → CommandResultText 终态回执（六态文案，见 specs/3.3-tui-cli.md 的「Reflection activity 展示规范」）+ activity 终态收口
      → ReflectionCompleted → DrainingInput → drain 收口 Completed
        （取消 / 超时则 Run 直接进入终态，见「取消 / 超时 / 失败语义」）
```

busy 丢弃的真实落点（裁决：busy NEVER 排队执行）：busy 期间 `ReflectNow` 先入队 session pending buffer 供 TUI 回显（#1816 快照），随后经 Runtime 侧两个丢弃点之一被剔除并逐条发布「已跳过本次手动触发」提示——①Run 收尾 `BufferedInputAdapter::drain_remaining_events` 回流 pending buffer 前（busy 积压的主拦截点）；②run_launch 循环顶 gate 消费 pending buffer 前（gate requeue 残留的拦截点）。两点共用 `input_gate::drop_queued_reflect_now`；idle 受理路径（idle_lifecycle gate）不经任一丢弃点。

Manual Run 硬约束（当前实现）：

- **不创建 RunStep、不调用主模型**：该 Run 只执行一次 Memory 反思。
- **不消费用户输入**：Run 期间 drain 出的 user message 经 `defer_user_batch` 回流 session 输入队列，由下一个会话 Run 消费；本 Run 走向 `EmptyAndSealed` 正常 `Completed`（Manual Compaction Run 同型），NEVER 因用户输入进入模型调用或 `Failed`。
- **不落盘**：不写 canonical session——`message_count`、`updated_at` 与 run slices 保持不变。
- **不递增 session 主 `run_count`、不发 `RunChanged`**：它不是用户回合，不消耗 Interval 频控计数。
- **busy 丢弃不排队**：busy 期间的 `ReflectNow` 在 Run 收尾回流点与 gate 消费 pending buffer 前被统一剔除并提示（`drop_queued_reflect_now`，与 `Compact` 的 busy 排队语义相反）；NEVER 排队到 Run 结束后执行第二次。
- **disabled 不创建 Run（创建前常态 no-op）**：配置门禁在创建 Run 前直接回 DisabledSkipped 文案，不创建 Run、不发布任何 activity——`DisabledSkipped` 常态下不是 Run/Activity 终态，而是创建前的 no-op。
- **消息快照来源**：idle 受理时经 `MainSessionWiring::bind_main_run` 读取 committed CanonicalSession 的 `structured_messages()`（与 `/sessions` 列表同一投影），即当前可见 active 历史；不包含 system 注入。
- **等待终态再回显**：handler await 反思终态，只回显安全文案/计数，NEVER 向 chat 投影反思正文。
- **两层门控**：input gate 的 busy 判定（Run 进行中直接丢弃）与配置门控（DisabledSkipped 文案）相互独立。

### 同步执行模型

三种 trigger 全部进入 Runtime-owned 的同一执行通道，调用方 await 到终态：`Completed`（携带不含正文的状态与计数）或 `DisabledSkipped`。执行开始前 append `Running` durable fact；成功、失败、partial apply、timeout 或 cancel 时再以同一 id `upsert` 终态。Runtime **NEVER** 把反思正文投影到 TUI 或 chat，只发布计数事实。`/reflect [limit]` 是只读 history query，不触发 LLM，也不执行 apply；其内容展示走显式内容投影（`ReflectionHistoryQuery::list_with_content` → `safe_summary_with_content`），偏差文本与建议内容仅在该查询路径跨 SDK 边界，默认 `list` 的 Safe 摘要保持零内容。

```text
Interval / PreCompact / Manual
  → engine 反思阶段（run_reflection_phase / execute_manual_reflection）
      ├─ BeginReflection → RunStatus::Reflecting（非终态）
      ├─ Reflection activity 发布（kind = Reflection，detail.trigger = Interval/PreCompact/Manual）
      └─ 端口执行:
          ├─ 配置禁用（判定后配置被关闭的竞态）→ DisabledSkipped（记录 [reflection_disabled]）；
          │   反思端口未绑定 → Interval/PreCompact 同样按 DisabledSkipped 跳过（warn 日志）
          │   （phase 内 DisabledSkipped 的 leaf 终态有差异：Interval/PreCompact 按
          │    Cancelled 收口；Manual 在端口层折叠为失败按 Failed 收口，其端口未绑定
          │    走端口 Err 路径先 Failed 收口再上抛——与创建前 no-op 不同）
          └─ append Running → build_prompt → call_llm → parse → optional apply
                            → upsert terminal record
                            → Reflection activity 按终态收口
                            → ReflectionCompleted 返回进入前状态
                            → 仅 Succeeded 且带 metadata 计 usage；
                              变更计数 > 0 时：TUI SystemMessage + 累积 LLM reminder
```

### Activity 结构化可见性

Reflection 的运行时可见性是结构化 Activity（SDK typed view），**不是**第二套业务状态机——业务事实以 Run 状态机与 Memory-owned `ReflectionRecord` 为准：

- **Run root**：`ActivityDetailView::Run { purpose }`，`purpose` 由 Run intent 映射为 `Main` 或 `Reflection`（见上文映射）。
- **Reflection leaf**：`ActivityKindView::Reflection` + `ActivityDetailView::Reflection { trigger }`，`trigger ∈ {Interval, PreCompact, Manual}`。Interval / PreCompact 归属当前对话 Run 的 activity 树；Manual 归属 purpose = Reflection 的 Run 根下（该 Run 没有 RunStep）。
- **状态集合**：`ActivityStateView` ∈ {`Running`, `Waiting`, `Succeeded`, `Failed`, `Cancelled`, `Terminated`}，终态为后四个。Reflection leaf 终态映射：反思 `Succeeded` → `Succeeded`、`Failed` → `Failed`、`Cancelled` → `Cancelled`、`TimedOut` → `Terminated`。
- **DisabledSkipped：创建前 no-op 与 phase 内终态的差异**：常态下 `DisabledSkipped` 在创建 Run 前的配置门禁就已返回——创建前常态 no-op，不创建 Run、不发布任何 activity。仅当配置在判定/受理后被关闭（竞态）或反思端口未绑定、Reflection leaf 已在 phase 内发布时，`DisabledSkipped` 才作为 phase 内 outcome 出现，且 leaf 终态按路径不同：Interval/PreCompact 的 shared phase 按 **`Cancelled`** 收口（状态机照常 Reflecting 往返）；Manual 在端口层将其折叠为失败、leaf 按 **`Failed`** 收口（Run 照常收口），Manual 反思端口未绑定则走端口 Err 路径先 `Failed` 收口再上抛错误。
- **Run root 终态**由 Run 状态映射：`Completed` → `Succeeded`、`Failed` → `Failed`、`Terminated` → `Terminated`；`Reflecting` 期间 Run root 保持 `Running`，leaf 先收口、root 后收口。
- **观测降级**：activity 发布/收口失败只记 warn 并继续执行（best-effort 观测，NEVER 阻断反思）；端口 Err 先把 activity 按 `Failed` 收口、状态机 `ReflectionCompleted` 返回后才上抛错误，NEVER 留下 Running 态 activity 或悬挂的 `Reflecting`。

### 完成时的记忆变更提示

反思 apply 产生变更时走双通道，两侧都在「反思完成」这一时刻发生：

- **TUI**：立即 `RuntimeStreamEvent::SystemMessage`，渲染为 system notice，文案只含条数。
- **LLM**：变更计数累积在 adapter 内，由**下一次 Main Run 启动时**取走一次，注入 `InvocationReminderData::MemoryUpdated`。当前轮的 LLM 请求在反思完成前已发出，因此只能进下一轮；这与 system prompt 的 Session 冻结语义一致。

零变更、失败、取消、超时与配置禁用都不产生任何提示。

### 归纳产物（跨条目结论）

反思除「提炼新记忆 / 标记过时」外，还判断「是否存在可归纳的组合」。沿用既有两步流程：prompt 要求模型输出 `synthesizes`（来源记忆的行序号）→ 解析映射为真实 id → apply 写入时置 `kind = Synthesized` 并把来源写入 `evidence`。

- **触发沿用既有三种**（Interval / PreCompact / Manual），不新增触发条件：归纳是「每 N 轮批量做」，存在最多 N 轮延迟；记忆是辅助信息源，当前对话 context 优先级更高，该延迟可接受。
- **M13 证据下限**：`evidence.len() >= 2`。单条来源的「归纳」等价于复制既有条目，MUST NOT 产出；此时**降级**为普通建议（`Raw` + 空 evidence），内容照常写入——丢弃模型产出的内容比降级更糟。
- **不新增产物类型**：归纳结论仍是 `MemoryEntry`，`kind` 表达来源、`MemoryCategory` 表达用途，两维正交。
- **「已被归纳吸收」是推导的**：读时反向查询「是否有 active 条目的 evidence 包含它」，NEVER 在原始记忆上加反向指针（与取代关系同一取舍）。
- **回滚**：模型停止输出 `synthesizes` 即退化为原行为，字段保持空。

**已知限制**：归纳质量取决于反思时可见的记忆集合。候选集过小只会**漏掉归纳机会**（不会产出错误结论）；本设计明确不引入「反思前的相关记忆检索」。

### 取消 / 超时 / 失败语义（当前实现）

三触发的取消入口（cancel 的语义是「cancel 当前执行单元，drain and settle」，NEVER 是 cancel run）：

| 触发 | 取消入口 | 执行单元载体 |
|---|---|---|
| Interval / PreCompact | TUI Esc / Ctrl-C → `AgentClient::cancel_current_run`（registry 有 active step 时走 CancelStep 协议）；或 root 取消联动 | engine 的 step token（`cancel.child_token()`）原样传入反思端口 |
| Manual | TUI Esc / Ctrl-C → `AgentClient::cancel_current_run`（registry 按 `RunIntent::ManualReflection` 只取消执行体 token，**NEVER** 置 `Terminate` control） | Run root token 经 `ActiveRunRegistry::activate_main` 注册，取消 root 联动反思执行体 |
| Conversation 的 step 间隙 | 同上入口返回 `NoActiveStep`（无执行单元可 cancel） | — |

- **Manual Reflection Run**：取消与超时是 Run 终态——取消经 root token 传入执行体、以 `Cancelled` 收口（Reflection leaf → `Cancelled`，Run → `Terminated(UserExit)`；NEVER 投影为 `SessionShutdown`，二者语义不同），`TimedOut` 经 `timeout_run`/`fail_run` 收口为 Run → `Failed`（timeout 按 fail 收口），Reflection leaf → `Terminated`；Run root activity 随 Run 终态收口（`TimedOut` 时随 `Failed` 收口）。
- **Interval / PreCompact**：反思的失败、取消与超时 **不终止宿主 Run**——Reflection leaf 按对应终态收口，状态机经 `ReflectionCompleted` 返回进入前状态后继续原流程；宿主 Run 的取消仍由既有 interrupt / step control 路径负责（反思复用该 step 的 cancellation token）。
- **取消的 history 落盘**：取消后 durable fact 以同一 stable id `upsert` 终态——`status = Failed`、`error_category = Cancelled`（NEVER 留下悬挂的 `Running`，也 NEVER 写入 `Succeeded`；冲突终态互斥）。
- **执行期间可取消**：取消只形成安全终态 metadata，不泄漏 prompt、provider raw response 或 Reflection 正文。
- **任务超时**：执行通道对反思施加 timeout（配置 `memory.reflection.timeout_secs`，默认 240s；装配期快照语义），超时形成安全终态（Manual 为 Run 终态；Interval / PreCompact 只收口 leaf activity）。
- **无后台残留**：三种 trigger 都在所属调用点 await 完成，Session teardown 不需要 drain 或等待后台 job。

### 执行可靠性与成本控制（当前实现）

- **反思专用模型**：配置 `memory.reflection.model`（selection 字符串）在 session 装配期解析一次并缓存到反思 adapter（三个触发点共享同一 `Arc`，每 session 只构建一次 provider）；解析失败 fail-open——记 warn 并以会话模型继续，NEVER 阻断主流程。未配置时行为与无该配置完全一致。
- **空响应有界重试**：provider 流结束但无文本（`EmptyResponse`）时用同一 prompt 重试 1 次；仍空则按 `Failed(EmptyResponse)` 收口。provider 错误（LlmCall）与取消路径 NEVER 重试。
- **解析失败有界修复（repair）**：调用 `complete` 之前先做不落盘的预解析；失败时把「原反思 prompt → 原始响应 → 纠错指令（附精确校验错误）」发回 provider 一次，修复响应可解析则以其完成 apply；修复调用失败或修复后仍非法时回退原始响应，按现状落 `Failed(Parse)`。上限 1 次、NEVER 递归；取消后跳过修复。
- **统计口径**：重试与 repair 的真实 token 消耗累加进本次反思的 usage 计数；duration 为整次执行的墙钟时间（含重试与 repair）。

### Usage 记账口径（当前实现）

- 仅反思 `Succeeded` 且携带 usage metadata 的终态记账；Interval / PreCompact / Manual 共用同一条共享成功记账路径（`record_successful_usage`），恰好落 1 条 `UsageRecordData` 计入 `/usage`。
- Manual Reflection Run 无真实 RunStep（`run_step_id = None`）：此时生成仅记账用的 `RunStepId`（UUIDv7）与 `ModelInvocationId` 作为记账 id；不发布 cost。Interval / PreCompact 携带宿主 Run 的真实 `run_step_id`。
- `Failed` / `Cancelled` / `TimedOut` / `DisabledSkipped` 不记账。retry / fallback 发生在执行层内部，只有最终 `Succeeded` 经过记账点，不重复记账。

### 计数口径

Interval 频控读数是 engine 执行态的 `step_count`；当前实现中会话 Run 启动时以 session `run_count` 播种该读数、主会话 Run 内不递增，因此命中条件等价于「每 `interval_runs` 个 Main Run」：

- 判定唯一入口在 engine 的 `ModelStep::Complete` 收尾路径（`record_model_invocation` 与 `ModelInvoked` 之后）；`ModelStep::Tools`（该跳有 tool call）从不判定。历史上独立的 `has_tool_calls` / `stop_reason` / `before_finish_gate_continue` 跳过参数已随执行点上移 engine 而删除。
- 同一 Run 内多个 `Complete` step（含 Stop Hook 等内部 continuation）共享该读数：engine 以 Run 级一次性闸门保证 Interval 反思在同一 Run 内至多执行一次——判定命中并即将开始反思 phase 时消耗闸门，未命中不消耗（后续判定仍可触发）；一旦开始 phase，端口错误/任务失败/取消也视为该 Run 已执行过，不再重复反思。
- `run_count` 在每个 Main Run 启动前递增（Manual Reflection Run 除外，见「Manual 显式入口链路」），`/clear` 与 resume 切换 session 时归零，NEVER 延续旧 session 计数。
- 门禁 `should_run_turn_reflection(config, step_count)`：memory 总开关、`reflection.enabled` 开启且 `interval_runs > 0`，并且 `step_count.is_multiple_of(interval_runs)`。

## 5. Prompt 构建（纯函数）

```rust
fn build_reflection_prompt(
    project_memory: &str,     // 当前项目记忆摘要
    recent_summary: &str,     // 最近对话摘要
    lang: &str,               // "zh" | "en"
) -> String;
```

Prompt 结构（i18n）：

```text
# 当前项目记忆
{project_memory}

# 最近对话摘要
{recent_summary}

# 任务
分析以上对话，识别：
1. deviations: agent 是否有偏离行为
2. suggested_memories: 值得持久化的新记忆
3. outdated_memories: 已过时的记忆（填本次列表行序号，如 M3）

只输出 JSON，格式如下：
{...}
```

- **project_memory**：从当前 Run 持有的同一 `MemoryPort` 读取 Project 层 active 条目，格式化为 `- [M1] [Category][tags] content` 列表（行序号即引用契约的索引）。
- **recent_summary**：从最近对话消息提取文本，按 `[User]/[Assistant]: text` 格式逆序拼接，截断到合理长度。
- **i18n**：prompt 模板支持中英文，按 `lang` 参数选择。

### memory_summary（纯函数）

```rust
fn memory_summary(entries: &[MemoryEntry]) -> String {
    entries.iter()
        .map(|e| format!("- [{:?}][{}] {}", e.category, e.tags.join(","), e.content))
        .collect::<Vec<_>>()
        .join("\n")
}
```

### recent_messages_summary（纯函数）

```rust
fn recent_messages_summary(messages: &[Message], max_chars: usize) -> String;
```

- 逆序遍历消息，提取 Text content block。
- 格式：`[User]: text` / `[Assistant]: text`。
- 截断到 `max_chars`（`usize::MAX` 表示不截断）。

## 6. Output 解析

```rust
fn parse_output(raw: &str) -> Result<ReflectionOutput, ReflectionError>;
```

- LLM 返回纯 JSON 文本。
- 使用 serde 反序列化为 `ReflectionOutput`。
- JSON 结构错误返回 `ReflectionError::Parse`，非 JSON 或无可提取对象返回 `ReflectionError::Unparseable`；两者只携带稳定的安全类别/固定描述，**NEVER** 附带 raw response、prompt、对话或 Reflection 正文。

### ReflectionError

```rust
enum ReflectionError {
    Parse,                                // JSON 解析失败，不携带原文
    Memory(MemoryError),                  // MemoryPort 操作失败
    InvalidSuggestion(String),            // suggestion 违反领域约束
    Unparseable,                          // 响应无法解析为 JSON，不携带原文
}
```

**注意**：Memory 的 `ReflectionError` 只表达 parse、领域约束和 Memory 操作失败，且 Display **NEVER** 附带 raw response、prompt、对话或正文。LLM 调用、空响应与执行期 apply 失败由 Runtime 的执行错误与安全类别表达，不能回流为 Memory parse 错误。

## 7. Apply 流程

```rust
async fn apply_output(
    output: &ReflectionOutput,
    memory: &dyn MemoryPort,
) -> Result<ReflectionApplyResult, MemoryError> {
    memory.apply_reflection(output).await
}
```

### 步骤

1. **apply_suggestions**：`MemoryPort::apply_reflection` 遍历 `suggested_memories`，将每条转换为 MemoryEntry（UUIDv7 + now + source=Llm），执行去重、容量判断与必要的归档重试。
2. **apply_outdated**：同一 Port 实例遍历 `outdated_memories` 并标记过期；无法解析的引用跳过并记录（NEVER 使整批失败）；它就是当前 Run shared lease 捕获的 active Memory Arc。
3. **提交语义**：每个 layer 使用 MemoryService candidate/CAS/publish 协议；跨层部分完成返回结构化 `MemoryError::PartialApply`，不伪装成全成功。

### auto_apply_suggestions

```rust
if config.reflection.auto_apply_suggestions {
    memory.apply_reflection(&output).await
}
```

- `auto_apply_suggestions = false`（默认）时，不修改 active Memory；完整 output 只作为 Memory-owned `ReflectionRecord` 持久化，`/reflect` 仍只返回安全摘要。
- `auto_apply_suggestions = true` 时，执行通道写入 suggestion 并标记过期；apply 计数进入 record / safe summary，并触发完成时的双通道计数提示（不投影正文）。

### ReflectionApplyResult

```rust
struct ReflectionApplyResult {
    suggestions_added: usize,     // 成功写入/合并的 suggestion 数
    outdated_marked: usize,       // 标记过期的记忆数
}
```

## 8. 完整编排流程（Runtime 侧）

Memory BC 提供 prompt / parse / apply / history 的单一 `ReflectionWorkflow`；Runtime 统一编排 Interval、PreCompact、Manual 三种 trigger 与 Provider 调用。三种 trigger 走同一条同步执行路径，状态与观测由 engine 的反思阶段（Interval/PreCompact 的 `run_reflection_phase`、Manual 的 `execute_manual_reflection`）收口，调用方 await 到终态。

```text
Runtime trigger（Interval / PreCompact / Manual）
  ├─ capture owned messages snapshot（Manual 在 run launch 装配前自 committed session 冻结）
  └─ engine 反思阶段
       ├─ BeginReflection → RunStatus::Reflecting（非终态）
       ├─ Reflection activity 发布（detail.trigger 对应三种来源）
       ├─ ReflectionTaskAdapter.run
       │    ├─ DisabledSkipped → [reflection_disabled] 安全日志，返回调用方
       │    └─ append Running → build_prompt → Provider invocation → parse_output
       │                      → optional MemoryPort.apply_reflection
       │                      → upsert terminal ReflectionRecord
       │                      → 返回 completion（状态 + 计数，不含正文）
       ├─ Reflection activity 按终态收口，ReflectionCompleted 返回进入前状态
       ├─ 仅 Succeeded 且带 metadata 计 usage
       └─ 计数 > 0 时发 TUI SystemMessage 并累积 LLM reminder

调用方（主 Run 的 ModelStep::Complete 收尾 / compact Committed 后的 Compacting 内 / Manual Reflection Run 的 DrainingInput）await 上述结果。
```

### 8.1 Pre-compact 快照语义

PreCompact 在 compact 前把所选 `messages` clone 为 owned snapshot，但只有 compact 成功产生 outcome 后才执行。compact 失败、被 hook block、消息不足或取消时不执行。执行只使用冻结快照，因此不会观察 compact 后的消息变化。

- **快照时机**：compact 执行前冻结，`messages` 尚未被压缩；执行时机在 `CompactOutcome::Committed` 之后、`CompactionCompleted` 转移之前——在 `Compacting` 内完成 `Compacting → Reflecting → Compacting` 往返后再放行压缩收口。
- **快照内容**：`messages_selected_for_precompact_memory(messages)` 的结果（只取 compact 会丢掉的消息）。
- **材料未暂存**：Skipped、反思端口未绑定或反思配置关闭时槽位为空/被丢弃，不进入反思 phase，也不影响压缩收口。
- **完成去向**：执行前 append `Running`，终态以同 id upsert；`Reflection` activity 在 `Compacting` 内发布并收口；不回传完整结果，也不在后续轮次 emit 正文。

### 8.2 History 与安全查询

`ReflectionRecord` 是 Memory-owned 持久化事实，包含 trigger、状态、可选 parsed output / apply result、错误类别、token usage 与 duration。Runtime 接受任务后先通过 `ReflectionHistoryStore::append` 写入 `Running`，成功、失败、partial apply、timeout 或 cancel 后以同 id `upsert` 终态；adapter 使用 project-scoped durable dataset，append/upsert/query 均由 Memory 拥有。它与运行时 `Reflection` activity 相互独立：activity 是本次执行的观测投影，`ReflectionRecord` 才是持久化历史。

`/reflect [limit]` 只调用 `ReflectionHistoryQuery::list(limit)`，该 query 已按 newest-first 返回至多 `limit` 条 `ReflectionSafeSummary`；Runtime/SDK 仅映射其交付 DTO：id、时间、trigger、status、deviation/suggestion/outdated 数量、apply 状态、错误类别、token 计数与耗时。该查询**不运行 Reflection、不 apply、也不返回 output 正文**。

### 8.3 安全日志

Reflection 日志只能记录 event、trigger、status、error category、token/count、duration 与 record id 等 metadata。日志 **NEVER** 包含 prompt、对话消息、Memory content、provider raw response、parsed output、formatted content 或任何正文截断；解析失败也不得记录所谓“前 N 字符”。

## 9. model 覆盖

```rust
struct ReflectionConfig {
    model: Option<String>,    // None = 继承主对话模型
}
```

- `None`：Reflection 使用与主对话相同的 LLM 模型。
- `Some("model-id")`：使用指定模型（如更便宜的模型跑反思）。
- Runtime 读取此配置，经 ProviderPort 选择对应 client。

## 10. 相关文档

- 模块入口：[README.md](README.md)
- 领域模型（MemoryEntry / MemorySuggestion）：[01-domain-model.md](01-domain-model.md)
- 检索与注入：[02-retrieval-and-injection.md](02-retrieval-and-injection.md)
- 端口与适配器（ReflectionWorkflow / history）：[04-ports-and-adapters.md](04-ports-and-adapters.md)
- Runtime 端口（ProviderPort）：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Context Map（Memory 不依赖 Provider）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)

## 修改历史

| 日期 | 变更 |
|---|---|
| 2026-10-01 | 文档与实现双向同步：三触发的 Run 归属与 `Reflecting` 状态路径（非终态、返回进入前状态、intent→purpose 映射）；Manual `/reflect-now` 创建 `ManualReflection` intent 的真实 Run（root purpose=Reflection、无 RunStep、不落盘、不递增 `run_count`/不发 `RunChanged`）；新增「Activity 结构化可见性」与「Usage 记账口径」；取消/超时/失败语义按当前实现改写；计数口径改为按当前判定位置描述；清理设计文档中的 Issue/PR 编号引用 |
| 2026-09-29 | 新增「归纳产物（跨条目结论）」小节：`synthesizes` → `kind=Synthesized` + `evidence`、M13 证据下限与降级语义、触发沿用既有三种、候选集为已知限制 |
| 2026-09-29 | 补充「计数口径」小节：区分 session 内 Main Run 序号 `run_count` 与 Run 内 LLM 跳数；Interval 判定只发生在回合收尾跳；`run_step` 日志字段只承载 LLM 跳数 |
| 2026-09-28 | 三种 trigger 由「单槽后台异步」改为「同一执行通道同步 await」：调用方 await 到终态，Run 的 cancellation token 传入执行通道，Session teardown 不再 drain；完成时按 apply 计数发 TUI SystemMessage 并累积下一轮 LLM reminder；`user_alert` 与无生产调用方的 `format_output` 随 i18n 死代码一并移除 |
| 2026-09-25 | 接通 Manual 显式入口：Tools catalog `/reflect-now` → SDK `ChatInputEvent::ReflectNow` → input gate（idle 受理 / busy 提示丢弃，NEVER 排队）→ run_launch handler 冻结 `structured_messages()` 快照启动手动反思 |
| 2026-07-20 | Run teardown 落地有界 drain→cancel→terminal 收口；Manual 显式入口从压缩事项中拆分 |
| 2026-07-20 | 接通 compact 成功后的 PreCompact 冻结快照提交 |
| 2026-07-20 | 将 parse 错误收窄为不含模型原文的稳定类别，且 `ReflectionHistoryQuery` 仅发布安全摘要；完整 record 保持在 Memory adapter 内部 |
| 2026-07-19 | 将旧 `MemoryStore` apply 示例更新为当前 Run shared lease 捕获的同一 `MemoryPort::apply_reflection`，保留 `ReflectionEngine` 作为无状态 prompt/parse 领域服务 |
| 2026-07-18 | 三 trigger Runtime 单槽异步、busy skip、静默完成、Memory-owned history append/query 持久化、`/reflect [limit]` 只读安全摘要、安全日志与 Run teardown drain/cancel timeout |
| 2026-07-12 | 初稿：ReflectionEngine 领域服务、MemorySuggestion、触发条件、prompt/output/apply、职责边界 |
| 2026-07-12 | 早期并发方案已由三 trigger 统一异步单槽语义取代 |
