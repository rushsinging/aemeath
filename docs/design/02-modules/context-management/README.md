# Context Management（支撑域）

> 层级：02-modules / context-management（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2）/ [#972](https://github.com/rushsinging/aemeath/issues/972)
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

**关键决策**：Session 是数据聚合，**不是 Agent 执行生命周期状态机**（该生命周期唯一由 Agent Runtime 的 Run 状态机表达）。Session 归 Context Management，不归 Agent Runtime。

## 2. 核心决策

1. **Session 是数据聚合，不是 Agent 执行状态机**：Session 持有对话历史（ChatChain），跨多次用户输入，是持久化单位（`/resume` 恢复的单位）。Agent 执行生命周期唯一由 Runtime 的 Run 状态机表达；Task / Workflow 等 BC 仍可拥有各自局部状态机。
2. **ContextPort OHS**：Context Management 通过 `ContextPort` 向 Runtime 开放能力，Runtime 只调 4 个方法（`build_window` / `needs_compaction` / `compact` / `append_and_persist`），不接触 Session 内部结构。
3. **Compact 五级管线**：L1 budget reduction 在 tool result 入 ChatChain 前限额；`build_window` 依次做 L2 snip / L3 microcompact / L4 context collapse 读模型变换；L5 auto-compact 才调用 LLM 并持久修改 ChatChain。编号与语义只以 [02-compact.md](02-compact.md) 为准。
4. **Token Budget 单一真相**：所有 token 估算、effective context window 公式、auto-compact 阈值常量收口于此，**NEVER** 散落到 Runtime 或 Provider。
5. **Prompt 组装内聚于 ContextPort**：系统提示组装是 async `build_window` 的内部步骤，由私有 `PromptPipeline` 完成。文件 Guidance 经 Context-owned `GuidanceSourcePort`，Skill 经供应方 `SkillMaterializationPort`；Prompt policy **NEVER** 直接读文件系统。
6. **Memory 检索经供应方 OHS**：Memory BC 独占检索、scoring、ranking 与 semantic retrieval；Context 的 `memory_inject` integration 经 `MemoryPort` 获取已排序条目，只负责 SystemBlock render / placement、token budget 与跨轮去重。
7. **跨 BC 快照组装**：Session 落盘时内嵌 Task / Project 快照（经端口收集，恢复时分发回去）——边界经端口，不共享内部结构。

## 3. Target 物理目录与六边形边界

Context Management 已有 Session、Compact、Token Budget、Prompt 与 Memory Injection 多个具备独立词汇、变化原因和测试夹具的稳定能力，因此竖切 **MUST** 收进 `capabilities/`，**NEVER** 平铺在 crate 根。crate 根只保留窄 façade、跨 capability 的真实 Published Language，以及仅供 Composition 使用的 opaque wiring：

```text
agent/features/context/src/
├── lib.rs                              # 窄 façade：ContextPort PL + composition-only wiring
├── capabilities.rs                    # 私有 capability 注册，不承载业务逻辑
└── capabilities/
    ├── session.rs                      # Session / ChatChain / identity façade
    ├── session/                        # append_step / envelope / persistence / resume（按证据展开）
    ├── compact.rs                      # 五级管线总协调
    ├── compact/                        # budget_reduction / snip / microcompact / collapse / auto
    ├── token_budget.rs                 # Token Budget 单一真相
    ├── prompt.rs                       # 私有 PromptPipeline
    ├── prompt/                         # guidance source / scan / render（按证据展开）
    └── memory_injection.rs             # render / placement / token budget / dedup
```

Context 与 Runtime **都遵循 Hexagonal**，但物理形状由能力证据决定，**NEVER** 复制同一目录模板。Runtime 当前只有一个完整 `agent_execution` 能力，适合在 crate 根使用轻量 `domain/application/ports/adapters`；Context 已有多个稳定能力，若按技术层横向铺开，会让一次 Session append、Prompt materialization 或 Compact 变化跨越多个顶层目录并掩盖 owner。因此 Context 先按 capability 组织，每个叶子再按真实 seam 局部应用 Hexagonal：

- `ContextPort` 是 Context-owned 入站 OHS，由 crate façade 发布；
- Session 持久化策略在 `session` 内拥有目的性 repository seam，技术 adapter 终止于该 seam；
- `GuidanceSourcePort` 靠近 `prompt` 消费策略；
- `MemoryPort`、`TaskPersist`、`WorkspacePersist` 等供应方 OHS 由对应 capability 的 integration 代码消费，**NEVER** 复制为 Context 同义 port；
- 只有多个稳定 port 或 adapter 已需要独立导航时，叶子内才 **MAY** 增加 `ports.rs` / 技术目录，**NEVER** 为对称预建。

当前 **NEVER** 创建 crate-root `shared/`。现有跨 capability 数据要么属于 crate façade 的 Published Language，要么已有明确 owner（Session、Token Budget、Prompt），要么来自其他 BC 的 Published Language；把它们抽到 `shared/` 会削弱所有权。未来只有某内容被至少两个稳定 capability 共同消费、没有更明确 owner、具有独立不变量且抽取后依赖方向仍正确时，才 **MAY** 建立最小 `shared.rs` + `shared/`；测试 fixture、Port、DTO 与暂时无法归类的代码 **NEVER** 作为创建依据。

该目录是 Context 的 Target 物理结构。小能力可继续保持单文件，只有共同变化与独立测试证据成立时才展开子目录；Context 的迁移期白名单与 Current 映射只在 [Migration Governance](../../03-engineering/03-migration-governance.md) 记录，本文不复制现行 guard shape。

## 4. 对外端口

| 端口 | 方向 | 消费方 | 职责 |
|---|---|---|---|
| `ContextPort` | Context-owned OHS（对外） | Agent Runtime | async `build_window` / `compact` / `append_and_persist` 与纯 `needs_compaction` |
| `GuidanceSourcePort` | Context-owned 出站 seam（私有消费） | PromptPipeline | async 物化 model / user guidance；隔离文件发现、canonical path、mtime cache 与 I/O |
| `SkillMaterializationPort` | Skill-owned OHS（消费） | PromptPipeline | async 返回已物化 Skill 文档；Context 不读 Skill 文件 |
| `MemoryPort` | Memory-owned OHS（消费） | ContextPort backing implementation | 检索当前 active Memory 供 Context Window 注入 |

> `PromptPipeline` 是私有具体 capability，不是第二个 OHS。只有 Guidance 文件 I/O 形成真实 volatile seam，才定义 `GuidanceSourcePort`；`MemoryPort` / `SkillMaterializationPort` 则由各供应 BC 发布。它们都不会经 `ContextPort` 暴露给 Runtime；Runtime 的 context_coordination 只依赖 `ContextPort`。

## 5. ContextPort 四方法

Runtime 与 Context Management 的上下文交互经 4 个方法：

| 方法 | 语义 | 内部步骤 |
|---|---|---|
| `build_window` | 构建本轮 Context Window | L2-L4 compact 读模型投影 → async Prompt/Skill 物化 → Memory → summary → 唯一 block 顺序；L1 已在 ToolResult 入链前完成 |
| `needs_compaction` | 是否需要压缩 | token budget 计算 → 返回 compaction urgency |
| `compact` | 执行 L5 持久压缩 | 在稳定 Session backing 上生成并提交 Compact segment；不暴露 `&mut ChatChain` |
| `append_and_persist` | 追加对话并落盘 | 写入 ChatChain → 收集跨 BC 快照 → 原子落盘 |

## 6. 与其他 BC 的关系

### Agent Runtime

Runtime 经 `ContextPort` 读写上下文。每个 RunStep 开始时调 `build_window`，结束时调 `append_and_persist`。Runtime 不接触 Session、ChatChain 或 compact 管线内部。

### Task / Project

Session 落盘时经 `TaskPersist::collect_snapshot()` / `WorkspacePersist::snapshot()` 收集快照，内嵌 Session DTO。恢复调用栈先取消并 join 全部 shared lease holder，自身不持 shared lease，再取得 owned exclusive session-switch lease；读取 Session、prepare Project → project-aware Config → Memory → Task、无失败 commit 与最终发布全在该同一 lease 内完成。Runtime / Tool 只获得同一 Task backing 的 `TaskAccess`，编译期无法调用 restore。精确协议见 [01-session.md](01-session.md) §7。

### Memory

Memory BC 经 `MemoryPort` 提供检索 / mutation 能力并独占 scoring、ranking 与 persistence error。Context Management 只把返回的已排序条目渲染、放置到 Context Window 并管理 token budget / 跨轮去重；Runtime 的 Reflection 编排以同一 active Memory Arc 写回 suggestion。

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
- **MUST** 启动 resume 与运行期 resume 复用同一 prepare / commit 协调器；任一 prepare 失败时 Session、Task、Workspace 与 active identity 全部不变。

## 8. 文档导航

| 文档 | 内容 |
|---|---|
| [01-session.md](01-session.md) | Session 聚合、ChatChain / ChatSegment、跨 BC 快照组装、恢复边界、会话身份管理 |
| [02-compact.md](02-compact.md) | Compact 五级管线（L1-L5）、策略分层、幂等性、非破坏优先 |
| [03-token-budget.md](03-token-budget.md) | Token 估算策略、effective context window 公式、auto-compact 阈值、幂等决策 |
| [04-prompt-guidance.md](04-prompt-guidance.md) | PromptPipeline、GuidanceSourcePort、Skill 物化、安全扫描覆盖、prompt cache 稳定性 |
| [05-memory-injection.md](05-memory-injection.md) | MemoryPort consumption、SystemBlock render / placement、token budget、跨轮 dedup、Reflection 时序 |

## 9. 相关文档

- 统一语言：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md) §3 Context Management
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md) §4 / §8 / §10
- Runtime 领域模型：[../runtime/01-domain-model.md](../runtime/01-domain-model.md)
- Runtime 恢复语义：[../runtime/05-recovery-semantics.md](../runtime/05-recovery-semantics.md)
- Memory BC：[../memory/README.md](../memory/README.md)
- Task BC：[../task/README.md](../task/README.md)
- Project BC：[../project/README.md](../project/README.md)
- 迁移治理：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：Context Management 模块入口、7 条核心决策、ContextPort OHS、四方法、跨 BC 快照组装 | #743 |
| 2026-07-13 | 补代码落点章节（`agent/features/context` crate + prompt 合并 + 目录映射表） | #762 |
| 2026-07-14 | 统一启动 / 运行期 resume 的跨 BC prepare-commit 协调与恢复后 Project identity 切换 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-15 | 冻结 Context Target 物理目录：多能力竖切收进 `capabilities/`，各叶子按真实 seam 局部应用 Hexagonal；当前无证据创建 crate-root `shared/` | [#868](https://github.com/rushsinging/aemeath/issues/868) |
