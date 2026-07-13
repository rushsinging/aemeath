# Context Management（支撑域）

> 层级：02-modules / context-management（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2）
> 本模块拥有对话历史容器（Session）、上下文压缩（Compact 家族）、token 预算计算、提示组装（Prompt/Guidance）与记忆注入（Memory Injection）。通过 `ContextPort` OHS 向 Agent Runtime 供给"构建本轮 Context Window"的完整能力。

## 1. 模块定位

Context Management 是 Agent Runtime 的"记忆中枢"——管理"喂给 LLM 什么上下文"。它持有对话历史、在 token 耗尽前压缩、组装系统提示、注入记忆，是 Runtime 之外最大的支撑域。

| 概念 | 回答 |
|---|---|
| **Session** | 对话历史容器是什么、怎么持久化 |
| **ChatChain / ChatSegment** | 对话历史怎么组织 |
| **Compact 家族** | token 不够时怎么回收 |
| **Token Budget** | 当前用了多少 token、是否需要 compact |
| **Prompt & Guidance** | 系统提示怎么组装 |
| **Memory 注入** | 记忆怎么检索和注入 |

**关键决策**：Session 是数据聚合，**不是状态机**（唯一状态机是 Agent Runtime 的 Run）。Session 归 Context Management，不归 Agent Runtime。

## 2. 核心决策

1. **Session 是数据聚合，不是状态机**：Session 持有对话历史（ChatChain），跨多次用户输入，是持久化单位（`/resume` 恢复的单位）。无 Session 状态机——唯一状态机是 Agent Runtime 的 Run。
2. **ContextPort OHS**：Context Management 通过 `ContextPort` 向 Runtime 开放能力，Runtime 只调 3 个方法（`build_window` / `needs_compaction` / `append_and_persist`），不接触 Session 内部结构。
3. **Compact 五级管线**：从零成本（规则）到高成本（LLM 摘要）逐级升级——L1 工具结果截断 / L2 micro-compact / L3 auto-compact / L4 context collapse / L5 LLM 摘要。策略分层，幂等决策。
4. **Token Budget 单一真相**：所有 token 估算、effective context window 公式、auto-compact 阈值常量收口于此，**NEVER** 散落到 Runtime 或 Provider。
5. **Prompt 组装内聚于 ContextPort**：系统提示组装是 `build_window` 的内部步骤，经 `PromptPort` 子端口完成。Guidance 解析、Skill 物化、安全扫描覆盖、prompt cache 稳定性统一管理。
6. **Memory 注入经 MemoryPort**：记忆检索与注入是 `build_window` 的内部步骤，经 `MemoryPort` 子端口完成。注入评分算法、semantic retrieval 演进路径统一管理。
7. **跨 BC 快照组装**：Session 落盘时内嵌 Task / Project 快照（经端口收集，恢复时分发回去）——边界经端口，不共享内部结构。

## 3. 模块内部结构

```text
context-management/
├── session/                # Session 聚合根
│   ├── aggregate.rs        # Session、SessionMetadata
│   ├── chat_chain.rs       # ChatChain + ChatSegment（对话历史链）
│   └── identity.rs         # 会话身份管理（列表、元数据、/resume）
├── compact/                # Compact 家族（五级管线）
│   ├── pipeline.rs         # 管线调度
│   ├── tool_result.rs      # L1 工具结果截断
│   ├── micro.rs            # L2 micro-compact
│   ├── auto.rs             # L3 auto-compact
│   ├── collapse.rs         # L4 context collapse
│   └── summary.rs          # L5 LLM 摘要
├── budget/                 # Token Budget（计算单一真相）
├── prompt/                 # Prompt & Guidance（PromptPort 子端口）
├── memory_inject/          # Memory 注入（MemoryPort 子端口）
├── port/                   # ContextPort trait（OHS）
└── api/                    # BC 对外 facade
```

目录表达业务能力而非 `contract / business / gateway / utils` 等横向技术层。Composition Root 是唯一生产装配入口。

### 代码落点

Context Management 是独立 BC，代码落在一个 crate：

**Crate 路径**：`agent/features/context`（workspace member）

**workspace 注册**：`Cargo.toml` → `members` 中包含 `agent/features/context`

**子模块 → crate 内目录映射**：

| 设计模块 | crate 内目录 | 来源（迁移前） |
|---|---|---|
| `session/` | `context/src/session/` | `runtime/business/session/` |
| `compact/` | `context/src/compact/` | `runtime/business/compact/` + `runtime/business/chat/looping/compact*.rs` |
| `budget/` | `context/src/budget/` | `runtime/business/compact/token_estimation.rs` |
| `prompt/` | `context/src/prompt/` | `agent/features/prompt/`（整体并入）+ `runtime/business/prompt/build/` |
| `memory_inject/` | `context/src/memory_inject/` | `runtime/business/chat/looping/memory_inject.rs` |
| `port/` | `context/src/port/` | 新建（ContextPort trait） |

> **prompt crate 合并**：原 `agent/features/prompt/` 在 #762 中整体并入 `context/src/prompt/`，workspace `members` 删除 `agent/features/prompt`。设计上 prompt 本就是 Context Management 内部子模块（非独立 BC），物理合并后 Runtime 只依赖一个 `context` crate。

> **依赖方向**：`runtime` crate → `context` crate → `sdk` + `provider`（ACL 隔离）+ `storage`（文件 I/O）。

## 4. 对外端口

| 端口 | 方向 | 消费方 | 职责 |
|---|---|---|---|
| `ContextPort` | OHS（对外） | Agent Runtime | `build_window` / `needs_compaction` / `append_and_persist` |
| `PromptPort` | 子端口（内部） | ContextPort | 系统提示组装、Guidance 解析、Skill 物化 |
| `MemoryPort` | 子端口（对外） | ContextPort + Memory BC | 记忆检索注入 + Reflection 写入 |

> `PromptPort` 和 `MemoryPort` 是 `ContextPort` `build_window` 的内部步骤，不直接暴露给 Runtime。Runtime 只依赖 `ContextPort`。

## 5. ContextPort 三方法

Runtime 与 Context Management 的全部交互经 3 个方法：

| 方法 | 语义 | 内部步骤 |
|---|---|---|
| `build_window` | 构建本轮 Context Window | L1-L4 compact → memory 注入 → prompt 组装 → 返回 messages |
| `needs_compaction` | 是否需要压缩 | token budget 计算 → 返回 compaction urgency |
| `append_and_persist` | 追加对话并落盘 | 写入 ChatChain → 收集跨 BC 快照 → 原子落盘 |

## 6. 与其他 BC 的关系

### Agent Runtime

Runtime 经 `ContextPort` 读写上下文。每个 RunStep 开始时调 `build_window`，结束时调 `append_and_persist`。Runtime 不接触 Session、ChatChain 或 compact 管线内部。

### Task / Project

Session 落盘时经 `TaskPort::collect_snapshot()` / `WorkspacePersist::snapshot()` 收集快照，内嵌 Session DTO。恢复时经端口分发回去。跨 BC 快照组装经端口，不共享内部结构。

### Memory

Memory BC 经 `MemoryPort` 提供检索注入能力。Context Management 调用 `MemoryPort` 检索记忆并注入 Context Window。Reflection 产出的 Memory Suggestion 也经 `MemoryPort` 写回 Memory BC。

### Provider

Provider 返回实际 API token 计数，Context Management 的 Token Budget 只做估算。Provider 的线格式经 ACL 隔离，不泄漏到 Context Management。

### Storage

Storage 提供原子写与损坏兜底**机制**，不拥有 Session 数据本体。Session 落盘经 Storage 的文件 I/O 能力。

## 7. 设计边界

- **NEVER** 让 Runtime 直接接触 Session、ChatChain 或 compact 管线内部结构。
- **NEVER** 将 token 预算常量或 auto-compact 阈值散落到 Runtime 或 Provider。
- **NEVER** 让 Provider 的线格式泄漏到 Context Management——经 ACL 隔离。
- **NEVER** 让 Session 驱动持久化——落盘由 Runtime 经 `ContextPort::append_and_persist` 触发。
- **MUST** 所有上下文构建经 `ContextPort` OHS。
- **MUST** compact 决策幂等（相同输入 → 相同决策）。
- **MUST** 跨 BC 快照组装经端口，不共享内部结构。

## 8. 文档导航

| 文档 | 内容 |
|---|---|
| [01-session.md](01-session.md) | Session 聚合、ChatChain / ChatSegment、跨 BC 快照组装、恢复边界、会话身份管理 |
| [02-compact.md](02-compact.md) | Compact 五级管线（L1-L5）、策略分层、幂等性、非破坏优先 |
| [03-token-budget.md](03-token-budget.md) | Token 估算策略、effective context window 公式、auto-compact 阈值、幂等决策 |
| [04-prompt-guidance.md](04-prompt-guidance.md) | PromptPort、Guidance 解析、Skill 物化、安全扫描覆盖、prompt cache 稳定性 |
| [05-memory-injection.md](05-memory-injection.md) | MemoryPort、注入评分算法、semantic retrieval 演进路径、Reflection 集成 |

## 9. 相关文档

- 统一语言：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md) §3 Context Management
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md) §4 / §8 / §10
- Runtime 领域模型：[../runtime/01-domain-model.md](../runtime/01-domain-model.md)
- Runtime 恢复语义：[../runtime/05-recovery-semantics.md](../runtime/05-recovery-semantics.md)
- Memory BC：[../memory/README.md](../memory/README.md)
- Task BC：[../task/README.md](../task/README.md)
- Project BC：[../project/README.md](../project/README.md)
- 迁移治理：[../../03-engineering/migration-governance.md](../../03-engineering/migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：Context Management 模块入口、7 条核心决策、ContextPort OHS、三方法、跨 BC 快照组装 | #743 |
| 2026-07-13 | 补代码落点章节（`agent/features/context` crate + prompt 合并 + 目录映射表） | #762 |
