# Context Management · Session 聚合

> 层级：02-modules / context-management（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2）/ #871 / [#972](https://github.com/rushsinging/aemeath/issues/972)
> 本文定义 Session 聚合——Context Management BC 的核心，对话历史的容器与持久化单位。**Session 属 Context Management，不属 Agent Runtime**（见 S1 决策：Session 是数据聚合，非状态机）。

## 1. 定位

Session 是**用户协作会话容器**——持有对话历史（喂给 LLM 的上下文本体），跨多次用户输入。

- **是数据聚合，不是 Agent 执行生命周期状态机**（该生命周期唯一由 Agent Runtime 的 Run 状态机表达；不否认其他 BC 的局部状态机）
- **是持久化单位**（`/resume` 恢复的单位）
- **主体是对话历史**，因此归 Context Management 而非独立 BC

## 2. Session 聚合

```rust
struct Session {                       // 聚合根（可序列化，持久化）
    id: SessionId,
    chats: ChatChain,                  // 对话历史链（见 §3）
    metadata: SessionMetadata,         // title/tags/notes/model；project 展示由 identity 派生
    tasks: TaskSnapshot,               // 跨 BC 快照（Task BC；Target writer 始终写出）
    workspace: PersistedWorkspaceContext, // Project PL；Target writer 始终写出
    created_at: Timestamp,
    updated_at: Timestamp,
}
```

> Session 的 Target **对话历史表示** **MUST** 只保留 `chats`（退役旧 `messages` 双轨），并继续包含 metadata 与 Task / Workspace snapshot。Project identity 的唯一权威是 `workspace.project_identity`；新格式 **NEVER** 写独立 `Session.cwd`。旧 wire DTO 的 `cwd` 只在兼容 reader 中存在，用于缺失 identity 的历史数据推导，**NEVER** 进入 live Session 聚合或被新 writer 输出。旧表示的退役责任、进度与退出条件见 [迁移治理](../../03-engineering/03-migration-governance.md)。

## 3. ChatChain / ChatSegment

```rust
struct ChatChain {                     // 活跃对话链（运行时管理器 + 持久化）
    segments: Vec<ChatSegment>,
    revision: SessionRevision,        // 单调递增版本号；每次 freeze_active / compact 后自增
}

type SessionRevision = u64;            // ChatChain 的稳定版本号，纳入 CompactionFingerprint

struct ChatSegment {                   // 对话链节点（实体）
    id: SegmentId,
    parent_id: Option<SegmentId>,      // Normal 指向前段；Compact 为 None（新链起点）
    kind: SegmentKind,                 // Normal | Compact
    summary: Option<String>,           // 仅 Compact 段，走 system 通道
    runs: Vec<CommittedRunSlice>,       // 保留 Run → RunStep → Message 结构
    compact_source_revision: Option<SessionRevision>, // 仅 Compact 段：基于哪一轮 backing revision 生成
    compact_committed: bool,           // 仅 Compact 段：是否已提交（用于幂等检查）
}

/// Session 中一个 Run 的已提交对话事实。
/// Normal segment 通常只有一个完整 Run；Compact segment 的 recent tail MAY
/// 包含多个 Run 的后缀 slice。
struct CommittedRunSlice {
    run_id: RunId,
    steps: Vec<CommittedRunStep>,
}

/// 一个 finalized RunStep 的 durable 对话投影。
struct CommittedRunStep {
    step_id: RunStepId,
    finalize_cause: FinalizeCause,
    messages: Vec<Message>,
    receipts: Vec<StepReceipt>,
}

enum SegmentKind { Normal, Compact }
```

`CommittedRunSlice` / `CommittedRunStep` 是 Context-owned 历史事实，不是 Runtime
状态机快照。它们只保留 stable identity、finalize cause 与已收口内容，不保存
`RunStatus` / `RunStepStatus`、future、cancellation scope 或 retry attempt。

### 3.1 ChatChain 方法

```rust
impl ChatChain {
    /// 当前版本号。
    fn revision(&self) -> SessionRevision;
    /// 返回活跃段的只读切片（compact 后为 [Compact 段]；否则为最近 Normal 段序列）。
    fn active_segments(&self) -> &[ChatSegment];
    /// 冻结当前活跃段，使后续 append / compact 在新段上操作。返回旧 revision。
    fn freeze_active(&mut self) -> SessionRevision;
    /// 提交 compact 结果：冻结旧链，写入 Compact 段（含 source_revision）。
    fn compact(
        &mut self,
        summary: String,
        recent_runs: Vec<CommittedRunSlice>,
        source_revision: SessionRevision,
    );
    /// 返回带 Run/Step 边界的活跃历史，供 snip / microcompact / compact 使用。
    fn structured_history(&self) -> Vec<&CommittedRunSlice>;
    /// 最终扁平读模型；只供 ContextWindow / Provider 出站投影使用。
    fn messages(&self) -> Vec<&Message>;
}
```

- **Normal 段**：一个完整 Run 的对话产出；其中包含一个或多个 finalized RunStep
- **Compact 段**：compact 产生的新链起点（`parent_id=None` + summary），并保存按完整 RunStep 保留的 recent tail；旧链冻结供审计
- Snip / Microcompact 以 `CommittedRunSlice` 为 Run 边界；recent tail 以 `CommittedRunStep` 为不可拆分边界
- `ChatChain::messages()` 是不可逆的最终派生投影，**NEVER** 作为 compact 切分、Run 保护窗口或 Step 识别的输入
- Provider 最终仍只接收有序 `Vec<Message>`，不感知 Run / RunStep；扁平化发生在 Context 完成 L2-L5 处理之后

## 4. 跨 BC 快照组装

Session 落盘时**内嵌** Task / Project 的 Published Snapshot。Context Management 直接消费 Project-owned `WorkspacePersist` 负责快照与 prepare-commit 恢复，并按 Context Window 用例消费 Project-owned `WorkspaceRead`；Runtime **NEVER** 中转这些能力，Composition **MUST** 从 active Main session slot 的同一 `CompositionWorkspaceScope` wiring 注入两种窄 view。Context Management **NEVER** 获得 `WorkspaceControl` 或 composition scope。

```
落盘：Context Management 经 TaskPersist.collect_snapshot() / WorkspacePersist.snapshot()
      收集 TaskSnapshot / PersistedWorkspaceContext → 内嵌 Session → 落盘
恢复：先收口 active lease holders，并取得排他 session-switch lease
      lease 内加载 / 升级 Session，再依次 prepare Project / Config / Memory / Task
      全部成功后，在同一排他 lease 内执行无失败 Project / Task commit
      再发布 Session / Memory / Config backing，Config watch 最后发布
上下文：构建 Context Window 时经 WorkspaceRead 读取 identity / root / path_base / branch 等稳定值
```

Session **拥有对话历史**，**MUST** 只内嵌其他 BC 发布的 DTO 副本，**NEVER** 共享内部状态或实现类型。`WorkspaceRead` **MUST** 只提供上下文数据，`WorkspacePersist` **MUST** 只提供持久化边界；两种能力 **NEVER** 合并成通用 workspace wrapper。

## 5. Session 与 Run 的关系

```
Session（对话历史容器，跨多次输入）
  └── ChatChain
        ├── ChatSegment
        │     └── Run #1 → Step #1 / Step #2 / ...
        ├── ChatSegment
        │     └── Run #2 → Step #1 / Step #2 / ...
        └── ...
```

- **一个 Session 含多个 Run 的对话产出**（Main 每次用户输入 → 一个 Run → 追加一个 Normal 段）
- **Run 读写 Session**：经 `ContextPort` 读历史构建 Context Window；每个 RunStep 经唯一 `StepFinalizer` 收口为 finalized projection 后追加并落盘到 Session
- Run 是内存态执行；Session 是持久化数据——两者生命周期不同（Run 短、Session 长）

## 6. RunStep 持久化边界

Session 采用 **per finalized RunStep** 提交：一个 RunStep 无论普通完成、接受 `CancelRunStep`，还是随 `TerminateRun` 收口，都先由 Runtime 唯一 `StepFinalizer` 形成不可变 finalized projection，再构造唯一 `ContextAppend`。Context Management 以 `(run_id, step_id)` 为幂等键追加 ChatChain、收集跨 BC snapshot 并原子落盘。`FinalizeCause` 只允许 `Completed | UserCancelledStep | RunTerminated`；它描述本次对话事实为何收口，不把 Run 状态机迁入 Session。

`append_and_persist` **MUST** 将 projection 追加到对应
`CommittedRunSlice.steps`，而不是只把 `messages` extend 到一个扁平数组。相同
`run_id` 的连续 Step 归入同一个 Run slice；新 `run_id` 创建新的 Normal segment。
旧 Session 缺少 Step 边界时，兼容 reader **MUST** 把一个 legacy Normal segment
视为一个不可拆分的 synthetic Step，**NEVER** 根据 role 或 ToolUse/ToolResult
顺序猜测真实 Step。

普通完成路径必须等待 model response 与该 response 发起的全部 Tool Call 收敛为 final outcome。控制路径则在共享绝对 deadline 内尽力收口，允许把已确认事实、finalizer 明确冻结的 partial assistant、deterministic Tool/Agent receipt 与 `CancellationUnconfirmed` 作为协议完整的 finalized projection 提交。**finalized partial Step 是已提交对话事实，不是可恢复的 Runtime 中间态。**

### 6.1 落盘内容

每次成功提交 **MUST** 保存：

- Session identity、完整 metadata、`created_at`、`updated_at` 与 schema version；
- ChatChain / ChatSegment、提交后的 `SessionRevision`；
- 当前 Step 已绑定并由 finalizer 纳入本次提交的 user inputs；尚在 InputBuffer、未绑定 Step 的内容不提升为 Session 事实；
- 普通完成时的完整 assistant message，或控制收口时由 finalizer 明确冻结的 partial assistant projection；原始 stream delta 不直接落盘；
- assistant 已声明的 Tool Calls，以及按原 ToolCall 顺序排列的协议完整 terminal results；
- 普通完成路径中每个 Tool Call 的最终 `ToolOutcome`，包括 Success、业务 Failure、Denied、Cancelled；
- 控制收口路径中的 deterministic Tool/Agent summary 和 terminal receipt；Safety receipt 至少保留 child/run/tool identity、terminal status、artifact references、可能副作用、未完成调用与 `CancellationUnconfirmed`，Full receipt再包含 completed actions、verified facts 与 remaining work；
- `FinalizeCause`、适用的 finalization detail，以及 Plan approval 等已经收敛为对话事实的 typed decision；
- 已提交 Compact segment、summary 与 source revision；
- `(RunId, RunStepId)` 幂等键、内容 fingerprint 与 committed revision/receipt；
- 本次提交时收集的 Task Published Snapshot 与 Workspace Published Snapshot；Target writer 即使为空也 **MUST** 显式写出。

`RunId` / `RunStepId` 在这里仅作为对话事实的关联与幂等 identity，**NEVER** 表示 Session 拥有或可恢复 Runtime 状态机。

### 6.2 不落盘内容

以下执行中间态 **NEVER** 进入 Session：

- Run 聚合、active RunSpec、RunStatus 与 RunStepStatus；`FinalizeCause` 是已提交 projection 的原因标签，不是状态机快照；
- ToolCall 的 PendingArgs、Ready、AwaitingApproval、Running 等进行态；finalizer 只保存其确定性 terminal projection/receipt；
- 原始 partial assistant stream 与 delta；只有控制收口时由 finalizer 冻结后的 partial assistant projection 才可落盘；
- 尚未被 finalizer 收敛或补齐 terminal result 的 Tool future、Tool suspension 与并发 batch；
- PendingInteraction、typed continuation、waiter、channel 与 UI 临时状态；
- cancellation token/scope、deadline、retry/backoff attempt 与 Provider HTTP/SSE 连接；
- RuntimeContext、各 Port 活实例、lease 与 Composition scope；
- StuckGuard / ToolLoopGuard 计数、临时 token 估算 cache；
- Sub Run 的执行状态与完整消息链；父 Agent Tool 只保存 child terminal receipt 形成的稳定 Tool result，**NEVER** 把 Sub 私有对话链注入父 Session；
- Stop Hook Block 后尚未获准提交的 assistant response 与 feedback 驱动的 pending Step。

### 6.3 并发 Tool 混合结果

同一 RunStep 的 Tool Call **MAY** 并发执行，但普通完成路径的收集与提交 **MUST NOT** fail-fast。Runtime 必须等待每个调用收敛为稳定 outcome，再按原 ToolCall 顺序构造单个 `ContextAppend`：

- 一个调用失败 **NEVER** 丢弃、回滚或覆盖同批其他调用已经成功的结果；
- 尤其 Agent Tool / Sub Run 已消耗大量 token 后成功返回时，即使兄弟 Agent Tool 失败，成功结果仍 **MUST** 作为该 Tool Call 的 final result 进入本 Step 并落盘，使下一次 model invocation 可直接复用；
- 失败调用也 **MUST** 以 typed Tool failure result 入链，让模型看到哪些工作失败及原因，而不是因整批返回 `Err` 丢失全部观察；
- completion 顺序只影响等待时机，**NEVER** 改变 Provider 协议顺序；最终消息顺序固定为 assistant tool calls → 原 ToolCall 顺序的 final results；
- L1 budget reduction 或大结果外置 **MAY** 改变成功结果的内联表示，但 **NEVER** 把成功事实变成缺失；外置时 Session 必须保存稳定引用与足够的摘要/metadata，供后续读取与判断；
- 只有 Context 自身的 revision conflict、内容冲突或 durable write 失败才使整个 `append_and_persist` 失败；单个 Tool 的业务 Failure 不属于 Session commit failure。

控制路径复用相同顺序与“成功事实不可丢”不变量，但不无限等待：`CancelRunStep` 最长 10s，`TerminateRun` 最长 5s，嵌套 Agent 共用控制请求创建的同一绝对 deadline。deadline 内成功返回的 Tool/Agent 结果 **MUST** 保留；未确认停止的调用由 finalizer 补齐 `CancellationUnconfirmed` receipt，而不是删除整个 batch。父 Step 取消对 Agent Tool 传播 child `TerminateRun`；父 Session 只保存 child terminal receipt 形成的 Tool result，不保存 child 完整消息链，也不为摘要额外调用 LLM。

如果进程在 finalized projection 完成原子提交前崩溃，该未提交 Step 不属于可恢复 Session；系统仍遵循“不持久化 Run 中间态”，**NEVER** 为保住进行中调用引入 Tool/Run checkpoint。这里的 finalized partial 只有在 `StepFinalizer` 收口且 `append_and_persist` 成功后才成为 durable 对话事实，不承诺崩溃下的 exactly-once。

### 6.4 提交与恢复语义

- 相同 `(run_id, step_id)` 与相同 fingerprint 的重试 **MUST** 幂等返回原 committed receipt；
- 相同幂等键但内容不同 **MUST** 返回 typed conflict，**NEVER** 覆盖已提交结果；
- durable handoff 前允许取消并转入 StepFinalizer；finalizer 完成 handoff 后提交由 cancellation-shielded owned task 跑到明确结果；
- commit 成功后 Runtime 才能 `mark_step_persisted`，普通完成或 finalized partial Step 从此都不再属于 cancellation rollback；
- `CancelRunStep` 提交成功后回 `DrainingInput`；`TerminateRun` 还必须 flush 已提交 Session content，未绑定 Step 的 InputBuffer **MAY** 丢弃；
- 恢复只加载最后一个完整 committed revision，其中可以包含已提交的 finalized partial Step。未提交 Step 不重放，新输入创建全新 Run。

## 7. Session 恢复边界

启动 resume 与运行期 `/resume` **MUST** 调用同一个联合恢复协调器：

1. resume coordinator **MUST** 先请求活动 Run 取消并 join Tool / Reflection / Sub 等全部 shared lease holder；发起 resume 的调用栈自身 **NEVER** 持有 shared lease，也 **NEVER** 尝试 shared → exclusive 原地升级。全部 holder 收口后 acquire owned exclusive `session-switch` lease，并从读取 / 升级 Session 开始一直持有到第 5 步发布完成；gate 释放前 **NEVER** 开始新 Run。Main Run admission 以及 Task / Workspace / Session 的外部 query、control、snapshot **MUST** 共享该 gate；恢复协调器持 exclusive permit 调用全部 participant，其他观察者因此无法看见 prepare token 与 commit 之间的状态变化。
2. 读取 Session 并在兼容 ACL 中升级旧 wire DTO。旧 Workspace snapshot 缺 identity / id 时以 canonical `LegacySessionDto.cwd` 推导；`workspace: None + cwd` 升级为 identity/root/path 均以 cwd 为基准、空 stack、派生 WorkspaceId 的完整 snapshot，**NEVER** 保留 active slot 的旧 Workspace。新格式若仍携带 legacy cwd，reader **MUST** 校验它与 `workspace.project_identity.initial_cwd` 一致，不一致则 prepare 失败。Target writer **MUST** 始终写 Workspace 与 Task snapshot；Task raw field 的 ID 格式与 legacy counter 升级只委托 Task-owned codec。legacy `tasks: None` **MUST** 调用 `TaskSnapshot::empty()` 得到 `tasks=[] / next_task_id=TaskId(1) / next_batch_id=BatchId(1) / current_batch=None / batches=[]` 并记录兼容诊断，**NEVER** 保留当前 active Session 的 Task。captured empty 保留其 wire 值，两者来源可诊断但 live 恢复结果同为显式空状态。
3. 在第 1 步取得的同一 exclusive lease 内，先调用 Project `prepare_restore`，只使用 `PreparedWorkspaceRestore::project_identity()` 返回的已验证 canonical identity，**NEVER** 直接信任 raw Session DTO identity。协调 ACL 把该 identity 映射为 Config-owned `ProjectConfigLocation`，再 await `ProjectConfigParticipant::prepare_for_project`；Config candidate **MUST** 加载目标项目层配置并验证 provider / tool / hook 所需输入。随后以同一 identity 与 `prepared_config.memory_config()` 调 `ProjectMemoryOpener::open_for_project`，最后调用 Task `prepare_restore`。prepare 完成全部可能失败的解析、I/O 与不变量校验，且 **NEVER** 修改 Session、Task、Workspace、Memory binding、Config active state 或 active identity；任一失败即丢弃全部 token / candidate resources并释放 gate。
4. 全部 participant 成功后进入无失败提交段：同步消费 `PreparedTaskRestore` 与 `PreparedWorkspaceRestore`，期间 **NEVER** await、执行 I/O 或响应取消。gate 阻止任何观察者读取多个 commit 之间的中间态。
5. Task / Workspace participant commit 后才发布 Session id / metadata / ChatChain，并把第 3 步准备的 Memory Arc 安装到 active Main session wiring；最后调用 Config participant 的无失败 `commit_project`，一次替换 Config-owned active `{location, snapshot}` 并发布 watch snapshot。随后释放 gate。下一 Main Run 从同一 shared lease 取得 Context、Task、Memory，并从同一 Config participant 读取 project-aware ConfigSnapshot；Provider / Tool / Hook factory **NEVER** 读取旧全局 snapshot。所有 fallible 工作 **MUST** 在第 3 步完成，**NEVER** 在提交段引入失败点。

进程若在内存提交段崩溃，Run 状态本就不持久化；重启后仍从未修改的持久化 Session source 重新走同一协调器，因此 **NEVER** 把半提交内存态发布为可恢复真相。该协议提供对 Runtime / Tool 观察者的原子切换语义，**NEVER** 假装跨多个锁存在底层数据库事务。

### 7.1 Context-owned MainSessionWiring

Context Management **MUST** 从 crate-root 窄 façade 发布仅供 Composition 调用的 opaque factory；这是 active Main session slot 的所有权真相，不意味着建立固定 `api/` 目录，Runtime 文档只展示接线：

```rust
context::wire_main_session(MainSessionDependencies {
    workspace_read: Arc<dyn WorkspaceRead>,
    workspace_persist: Arc<dyn WorkspacePersist>,
    task_persist: Arc<dyn TaskPersist>,
    memory_opener: Arc<dyn ProjectMemoryOpener>,
    initial_memory: Arc<dyn MemoryPort>,
    config: Arc<dyn ProjectConfigParticipant>,
    guidance_source: Arc<dyn GuidanceSourcePort>,
    skill_materialization: Arc<dyn SkillMaterializationPort>,
    // Session repository / config 省略
}) -> Result<MainSessionWiring, SessionOpenError>

MainSessionWiring::bind_main_run(&self).await
    -> Result<BoundMainRun, SessionSwitchInProgress>

struct BoundMainRun {
    context: Arc<dyn ContextPort>,
    memory: Arc<dyn MemoryPort>,
    config: ConfigSnapshot,
    lease: MainRunLease,
}
```

`MainSessionWiring` 字段私有，拥有稳定 Session backing、唯一 `SessionSwitchCoordinator`、async shared / exclusive gate、`TaskPersist`、active Memory slot、Config participant view，以及构造私有 PromptPipeline 所需的稳定 Guidance / Skill seam；它 **NEVER** 保存第二份 active Config slot。Guidance / Skill adapter 每次按 request 的 project/config materialize，因而 resume 后 **NEVER** 静态捕获旧项目内容。`bind_main_run` **MUST** 是 async admission：await 一个 owned shared lease 后，才从 Memory slot 与 Config participant 的同一已提交版本读取资源并构造 run-bound `ContextPort` view；它 **NEVER** 用同步 read lock 跨越 resume 的 await。该 ContextPort 复用稳定 Session / ChatChain backing，但只捕获本 lease 对应的 Memory Arc 与 ConfigSnapshot，**NEVER** 静态捕获启动时实例。`BoundMainRun` 的资源不能越过 lease 存活。

`MainSessionWiring::resume` 是唯一 exclusive project-switch 入口，执行 §7 的 prepare / commit 协议；可能改变 active project-scoped resource 的 Config command 也 **MUST** 经 wiring 使用同一 gate 与 candidate protocol。ordinary ContextPort、Runtime 与 Tool **NEVER** 获得 coordinator、active slot setter、`TaskPersist`、Config participant commit authority 或 exclusive lease。无 Run 的 Session / Memory / Workspace / Config query 或 mutation 也必须经 gate-aware async façade await 同一 owned shared lease；因此同一 gate 同时证明 Run admission、资源读取与 resume 的原子边界。

Config update 还有 durable state：在 Config / Memory candidate 均 prepare 后，wiring **MUST** 把 owned exclusive permit 与全部 candidate 一次性交给 cancellation-shielded owned task。handoff 是最后一个取消点；一旦开始 durable publish，即使调用方 future 被丢弃，owned task 仍必须跑完 fallible persist，并在成功后无 await 地依次安装 Memory、提交 Config active state、最后发布 Config watch。逐 await / rename / fsync / commit 点的故障注入与二选一不变量见 [Config §5.3](../config/01-config-layer.md#53-config-update-的联合协议)。

## 8. 会话身份管理

Context Management 还负责会话 identity：session 列表、元数据、`/resume` 选择、切换。这是**数据管理，不是状态机**。

## 9. 相关文档

- Run 聚合（读写 Session）：[../runtime/01-domain-model.md](../runtime/01-domain-model.md)
- 恢复语义：[../runtime/05-recovery-semantics.md](../runtime/05-recovery-semantics.md)
- Compact 家族（ContextPort OHS）：[02-compact.md](02-compact.md)
- Token Budget：[03-token-budget.md](03-token-budget.md)
- Prompt & Guidance：[04-prompt-guidance.md](04-prompt-guidance.md)
- Memory 注入：[05-memory-injection.md](05-memory-injection.md)
- 上下文地图（Session 属 Context Management）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- Project Workspace 端口：[../project/02-ports-and-adapters.md](../project/02-ports-and-adapters.md)
- 代码组织规范：[../../01-system/06-code-organization.md](../../01-system/06-code-organization.md)
- 迁移治理：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)
- 统一语言（Session/ChatChain/ChatSegment）：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：Session 聚合、ChatChain/ChatSegment、跨 BC 快照组装、与 Run 关系、恢复边界 | #761 |
| 2026-07-12 | 补充 ContextPort 相关文档交叉引用 | #786 |
| 2026-07-14 | Session 快照组装改为直接消费 Project-owned WorkspacePersist；以联合 prepare / gate 内无失败 commit 原子切换 Task、Workspace、Memory 与 Session identity，并复用 active Main session slot scope | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-15 | 明确 per finalized RunStep 的落盘/不落盘矩阵、幂等提交与恢复语义；普通 mixed outcomes 与控制收口都必须保留同批成功 Agent Tool 结果，并以 deterministic receipt 表达 cancelled/unconfirmed 工作 | [#868](https://github.com/rushsinging/aemeath/issues/868) / [#700](https://github.com/rushsinging/aemeath/issues/700) |
| 2026-07-17 | ChatChain backing 保留 Run → finalized RunStep → Message 结构；compact recent tail 按 Step，扁平化延后到 ContextWindow / Provider 出站 | compact token reset design |
