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

> Session 的 Target **对话历史表示** **MUST** 只保留 `chats`（退役旧 `messages` 双轨），并继续包含 metadata 与 Task / Workspace snapshot。Project identity 的唯一权威是 `workspace.project_identity`；新格式 **NEVER** 写独立 `Session.cwd`。旧 wire DTO 的 `cwd` 只在兼容 reader 中存在，用于缺失 identity 的历史数据推导，**NEVER** 进入 live Session 聚合或被新 writer 输出。旧表示的退役责任、进度与退出条件见 [迁移治理](../../03-engineering/migration-governance.md)。

## 3. ChatChain / ChatSegment

```rust
struct ChatChain {                     // 活跃对话链（运行时管理器 + 持久化）
    segments: Vec<ChatSegment>,
}

struct ChatSegment {                   // 对话链节点（实体）
    id: SegmentId,
    parent_id: Option<SegmentId>,      // Normal 指向前段；Compact 为 None（新链起点）
    kind: SegmentKind,                 // Normal | Compact
    summary: Option<String>,           // 仅 Compact 段，走 system 通道
    messages: Vec<Message>,            // Shared Kernel VO
}

enum SegmentKind { Normal, Compact }
```

- **Normal 段**：一条 user 消息 + 其触发的完整回合（对应一个 Run 的对话产出）
- **Compact 段**：compact 产生的新链起点（`parent_id=None` + summary），旧链冻结保留供审计
- `ChatChain` 提供扁平 `messages()` 读模型供 Loop Engine 的 context_coordination 构建 Context Window

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
        ├── ChatSegment (Run #1 的对话产出)
        ├── ChatSegment (Run #2 的对话产出)
        └── ...
```

- **一个 Session 含多个 Run 的对话产出**（Main 每次用户输入 → 一个 Run → 追加一个 Normal 段）
- **Run 读写 Session**：经 `ContextPort` 读历史构建 Context Window；每个 RunStep 结束后对话追加并落盘到 Session
- Run 是内存态执行；Session 是持久化数据——两者生命周期不同（Run 短、Session 长）

## 6. 恢复边界

- **落盘**：ChatChain（每个 RunStep 结束后落盘）+ 内嵌 Task/Workspace 快照
- **不落盘**：Run 执行状态（内存态）
- **恢复语义**：加载 Session 恢复**对话历史**，新输入开**全新 Run**（从头开始）——见 [../runtime/05-recovery-semantics.md](../runtime/05-recovery-semantics.md)

启动 resume 与运行期 `/resume` **MUST** 调用同一个联合恢复协调器：

1. resume coordinator **MUST** 先请求活动 Run 取消并 join Tool / Reflection / Sub 等全部 shared lease holder；发起 resume 的调用栈自身 **NEVER** 持有 shared lease，也 **NEVER** 尝试 shared → exclusive 原地升级。全部 holder 收口后 acquire owned exclusive `session-switch` lease，并从读取 / 升级 Session 开始一直持有到第 5 步发布完成；gate 释放前 **NEVER** 开始新 Run。Main Run admission 以及 Task / Workspace / Session 的外部 query、control、snapshot **MUST** 共享该 gate；恢复协调器持 exclusive permit 调用全部 participant，其他观察者因此无法看见 prepare token 与 commit 之间的状态变化。
2. 读取 Session 并在兼容 ACL 中升级旧 wire DTO。旧 Workspace snapshot 缺 identity / id 时以 canonical `LegacySessionDto.cwd` 推导；`workspace: None + cwd` 升级为 identity/root/path 均以 cwd 为基准、空 stack、派生 WorkspaceId 的完整 snapshot，**NEVER** 保留 active slot 的旧 Workspace。新格式若仍携带 legacy cwd，reader **MUST** 校验它与 `workspace.project_identity.initial_cwd` 一致，不一致则 prepare 失败。Target writer **MUST** 始终写 Workspace 与 Task snapshot；Task raw field 的 ID 格式与 legacy counter 升级只委托 Task-owned codec。legacy `tasks: None` **MUST** 调用 `TaskSnapshot::empty()` 得到 `tasks=[] / next_task_id=TaskId(1) / next_batch_id=BatchId(1) / current_batch=None / batches=[]` 并记录兼容诊断，**NEVER** 保留当前 active Session 的 Task。captured empty 保留其 wire 值，两者来源可诊断但 live 恢复结果同为显式空状态。
3. 在第 1 步取得的同一 exclusive lease 内，先调用 Project `prepare_restore`，只使用 `PreparedWorkspaceRestore::project_identity()` 返回的已验证 canonical identity，**NEVER** 直接信任 raw Session DTO identity。协调 ACL 把该 identity 映射为 Config-owned `ProjectConfigLocation`，再 await `ProjectConfigParticipant::prepare_for_project`；Config candidate **MUST** 加载目标项目层配置并验证 provider / tool / hook 所需输入。随后以同一 identity 与 `prepared_config.memory_config()` 调 `ProjectMemoryOpener::open_for_project`，最后调用 Task `prepare_restore`。prepare 完成全部可能失败的解析、I/O 与不变量校验，且 **NEVER** 修改 Session、Task、Workspace、Memory binding、Config active state 或 active identity；任一失败即丢弃全部 token / candidate resources并释放 gate。
4. 全部 participant 成功后进入无失败提交段：同步消费 `PreparedTaskRestore` 与 `PreparedWorkspaceRestore`，期间 **NEVER** await、执行 I/O 或响应取消。gate 阻止任何观察者读取多个 commit 之间的中间态。
5. Task / Workspace participant commit 后才发布 Session id / metadata / ChatChain，并把第 3 步准备的 Memory Arc 安装到 active Main session wiring；最后调用 Config participant 的无失败 `commit_project`，一次替换 Config-owned active `{location, snapshot}` 并发布 watch snapshot。随后释放 gate。下一 Main Run 从同一 shared lease 取得 Context、Task、Memory，并从同一 Config participant 读取 project-aware ConfigSnapshot；Provider / Tool / Hook factory **NEVER** 读取旧全局 snapshot。所有 fallible 工作 **MUST** 在第 3 步完成，**NEVER** 在提交段引入失败点。

进程若在内存提交段崩溃，Run 状态本就不持久化；重启后仍从未修改的持久化 Session source 重新走同一协调器，因此 **NEVER** 把半提交内存态发布为可恢复真相。该协议提供对 Runtime / Tool 观察者的原子切换语义，**NEVER** 假装跨多个锁存在底层数据库事务。

### 6.1 Context-owned MainSessionWiring

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

`MainSessionWiring::resume` 是唯一 exclusive project-switch 入口，执行 §6 的 prepare / commit 协议；可能改变 active project-scoped resource 的 Config command 也 **MUST** 经 wiring 使用同一 gate 与 candidate protocol。ordinary ContextPort、Runtime 与 Tool **NEVER** 获得 coordinator、active slot setter、`TaskPersist`、Config participant commit authority 或 exclusive lease。无 Run 的 Session / Memory / Workspace / Config query 或 mutation 也必须经 gate-aware async façade await 同一 owned shared lease；因此同一 gate 同时证明 Run admission、资源读取与 resume 的原子边界。

Config update 还有 durable state：在 Config / Memory candidate 均 prepare 后，wiring **MUST** 把 owned exclusive permit 与全部 candidate 一次性交给 cancellation-shielded owned task。handoff 是最后一个取消点；一旦开始 durable publish，即使调用方 future 被丢弃，owned task 仍必须跑完 fallible persist，并在成功后无 await 地依次安装 Memory、提交 Config active state、最后发布 Config watch。逐 await / rename / fsync / commit 点的故障注入与二选一不变量见 [Config §5.3](../config/01-config-layer.md#53-config-update-的联合协议)。

## 7. 会话身份管理

Context Management 还负责会话 identity：session 列表、元数据、`/resume` 选择、切换。这是**数据管理，不是状态机**。

## 8. 相关文档

- Run 聚合（读写 Session）：[../runtime/01-domain-model.md](../runtime/01-domain-model.md)
- 恢复语义：[../runtime/05-recovery-semantics.md](../runtime/05-recovery-semantics.md)
- Compact 家族（ContextPort OHS）：[02-compact.md](02-compact.md)
- Token Budget：[03-token-budget.md](03-token-budget.md)
- Prompt & Guidance：[04-prompt-guidance.md](04-prompt-guidance.md)
- Memory 注入：[05-memory-injection.md](05-memory-injection.md)
- 上下文地图（Session 属 Context Management）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- Project Workspace 端口：[../project/02-ports-and-adapters.md](../project/02-ports-and-adapters.md)
- 代码组织规范：[../../01-system/06-code-organization.md](../../01-system/06-code-organization.md)
- 迁移治理：[../../03-engineering/migration-governance.md](../../03-engineering/migration-governance.md)
- 统一语言（Session/ChatChain/ChatSegment）：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：Session 聚合、ChatChain/ChatSegment、跨 BC 快照组装、与 Run 关系、恢复边界 | #761 |
| 2026-07-12 | 补充 ContextPort 相关文档交叉引用 | #786 |
| 2026-07-14 | Session 快照组装改为直接消费 Project-owned WorkspacePersist；以联合 prepare / gate 内无失败 commit 原子切换 Task、Workspace、Memory 与 Session identity，并复用 active Main session slot scope | [#972](https://github.com/rushsinging/aemeath/issues/972) |
