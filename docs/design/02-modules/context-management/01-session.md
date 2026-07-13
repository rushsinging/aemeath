# Context Management · Session 聚合

> 层级：02-modules / context-management（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2）
> 本文定义 Session 聚合——Context Management BC 的核心，对话历史的容器与持久化单位。**Session 属 Context Management，不属 Agent Runtime**（见 S1 决策：Session 是数据聚合，非状态机）。

## 1. 定位

Session 是**用户协作会话容器**——持有对话历史（喂给 LLM 的上下文本体），跨多次用户输入。

- **是数据聚合，不是状态机**（无 Session 状态机；唯一状态机是 Agent Runtime 的 Run）
- **是持久化单位**（`/resume` 恢复的单位）
- **主体是对话历史**，因此归 Context Management 而非独立 BC

## 2. Session 聚合

```rust
struct Session {                       // 聚合根（可序列化，持久化）
    id: SessionId,
    cwd: String,
    chats: ChatChain,                  // 对话历史链（见 §3）
    metadata: SessionMetadata,         // title/tags/notes/model/project 等
    tasks: Option<TaskSnapshot>,       // 跨 BC 快照（Task BC，见 §4）
    workspace: Option<PersistedWorkspaceContext>, // Project Published Language
    created_at: Timestamp,
    updated_at: Timestamp,
}
```

> Session 的 Target 持久化表示 **MUST** 只保留 `chats`。旧表示的退役责任、进度与退出条件见 [迁移治理](../../03-engineering/migration-governance.md)。

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

Session 落盘时**内嵌** Task / Project 的 Published Snapshot。Context Management 直接消费 Project-owned `WorkspacePersist` 负责快照与恢复，并按 Context Window 用例消费 Project-owned `WorkspaceRead`；Runtime **NEVER** 中转这些能力，Composition **MUST** 从当前 `CompositionRunScope` 的同一 Project wiring 注入两种窄 view。Context Management **NEVER** 获得 `WorkspaceControl` 或 composition scope。

```
落盘：Context Management 经 TaskPort.snapshot() / WorkspacePersist.snapshot()
      收集 TaskSnapshot / PersistedWorkspaceContext → 内嵌 Session → 落盘
恢复：加载 Session → 经 TaskPort.restore() / WorkspacePersist.restore()
      将 Published Snapshot 分发回 Task / Project BC
上下文：构建 Context Window 时经 WorkspaceRead 读取 root / path_base / branch 等稳定值
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
| 2026-07-14 | Session 快照组装改为直接消费 Project-owned WorkspacePersist，并以 WorkspaceRead 提供 Context Window 数据 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
