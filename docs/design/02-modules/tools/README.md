# Tool & Skill & Command（支撑域）

> 层级：02-modules / tools（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#787（S2）
> 本模块拥有模型工具、提示技能与用户 Slash Command 的发现和调用语义。三类能力属于同一 BC，但**不共享执行抽象**。

## 1. 模块定位

Tool、Skill、Slash Command 共享“向 Agent 暴露可发现能力”的业务语境，但执行机制不同：

| 能力 | 发起者 | 核心语义 | 主要下游 |
|---|---|---|---|
| **Tool** | 模型 | 调用函数或外部能力，返回结构化 ToolOutcome | Agent Runtime |
| **Skill** | 模型 / 配置 | 将可复用提示资产物化为 PromptFragment | Context Management |
| **Slash Command** | 用户 | 注入 Prompt、查询 Snapshot 或调用应用命令 | 各目标 BC |

三者**不得**统一成 `Capability::execute()`：Tool 是函数调用，Skill 主要是 prompt 注入，Slash Command 可能直接查询或改变应用状态。模块级 facade 只负责命名空间与装配，不承载统一执行语义。

## 2. 核心决策

1. **共享 BC，不共享执行抽象**：分别使用 Tool、Skill、Command 的 Published Language 与端口。
2. **Tool 双端口**：目录查询与函数执行分为 `ToolCatalogPort`、`ToolExecutionPort`，Runtime 不接触 Registry 或 Tool 实例。
3. **Scope 与 Profile 正交**：Registry Scope 决定装配了什么；Tool Profile 决定允许什么能力。
4. **权限只收缩**：有效工具集为 `Registry Scope ∩ Profile Allowed Capabilities`，Profile 不能扩展 Scope。
5. **MCP 是 Tool Adapter**：不是独立 BC，也不与 Skill/Command 平级。
6. **Runtime 拥有调用编排**：Policy、Hook、审批、timeout、并发、重试与结果写入 Run Step 均归 Runtime Tool Coordination。
7. **Tool BC 守护局部不变量**：存在性、Scope、Profile、schema 与函数调用不能被调用方绕过。
8. **Context Management 拥有 Context Window**：Skill 与 PromptInjection Command 只提供 PromptFragment，不直接改 System Prompt。
9. **Tool 身份保持最小化**：使用规范化 ToolName；稳定 ID、版本和重命名兼容属于 MCP 动态接线阶段的独立决策。

## 3. Target 物理目录

Tool、Skill、Command 与 MCP 的 Target 依赖方向是 Hexagonal + Clean（`domain ← application ← ports ← adapters`）。Tool catalog 与 execution 是同一 Tool 能力的两个窄入口，共置于 `domain` / `application`；MCP 是 Tool 的外部协议 adapter，其连接状态机和协议测试已形成独立变化轴，因此作为 `adapters/mcp/` 子目录存在并只向 Tool façade 投影。

> **当前落地（[#993](https://github.com/rushsinging/aemeath/issues/993)）**：本 crate 已完成物理目录迁移，按证据启用 `domain + adapters` 两层过渡形态；`application/` 与 `ports/` 层暂不铺设，待 T1-T12 语义 leaf 需要时按证据启层。`ToolCatalogPort` / `ToolExecutionPort` 等对外端口尚未抽出——当前 `Tool` / `TypedTool` / `ToolListProvider` / `AgentRunner` 等 trait 暂居 `domain/`，随 T1/T5 语义 leaf 再抽入 `ports/`。crate-root `lib.rs` 是窄 façade（只再导出 PL 与 composition-only wiring）；原 `shared/tool` 的 Published Language 类型已迁入 `domain/types` 并经 façade 复出。

已落地的过渡物理目录（`domain + adapters`）：

```text
src/
├── lib.rs                             # 窄 façade：PL 再导出 + composition-only wiring
├── domain/                            # 领域策略、不变量、Published Language
│   ├── tool.rs                        #   Tool / TypedTool / ToolListProvider trait（端口暂居 domain）
│   ├── tool_types.rs                  #   ToolOutcome / ToolResult / PolicyDecision 等 PL
│   ├── agent_port.rs                  #   AgentRunner trait（端口暂居 domain）
│   ├── context.rs / resources.rs      #   ToolExecutionContext / ToolResources
│   └── types/                         #   自 shared/tool 迁入的 PL DTO
└── adapters/                          # 技术实现、外部 detail
    ├── …（builtin tool 实现：bash / file_read / grep / lsp / web_fetch …）
    ├── registry.rs / wiring.rs        #   catalog 装配
    └── mcp/ , mcp_manager/            #   MCP ACL + connection façade（transport / protocol / projection）
```

语义 leaf（T1-T12）铺齐后收敛到 Target 依赖方向：`domain ← application ← ports ← adapters`——`application/` 承载 Tool execute / Skill materialize / Command route 用例，`ports/` 承载 `ToolCatalogPort` / `ToolExecutionPort` / `SkillCatalogPort` / `SkillMaterializationPort` / `CommandCatalogPort` / `CommandRouterPort`。

MCP wire DTO、transport 与认证不得泄漏出 `adapters/mcp/`。Composition Root 是唯一生产装配入口。

## 4. 对外端口

| 端口 | 消费方 | 职责 |
|---|---|---|
| `ToolCatalogPort` | Runtime | 返回当前 Scope/Profile 下的 ToolCatalogSnapshot；Runtime 每次 invocation 冻结一次模型 schema 投影并经 ContextRequest 传给 Context Management |
| `ToolExecutionPort` | Runtime Tool Coordination | 校验并调用一个 Tool，返回 ToolOutcome |
| `SkillCatalogPort` | Runtime / Context Management | 发现 SkillDescriptor |
| `SkillMaterializationPort` | Context Management | async 物化当前可用 Skill，返回 PromptFragment 集合 + revision |
| `CommandCatalogPort` | CLI / TUI / Server | 发现和补全 Slash Command |
| `CommandRouterPort` | 交付层 | 路由 PromptInjection、SnapshotQuery、ApplicationControl |

这些端口可由同一 BC facade 暴露，但不得返回内部 Registry、Tool 实例、MCP client 或 RuntimeContext。

## 5. 与其他 BC 的关系

### Agent Runtime

Runtime 通过 `ToolCatalogPort` 获取模型可见 schemas，通过 `ToolExecutionPort` 调用函数。Runtime 自己编排 Policy、Hook、审批、timeout、并发、取消、失败策略和 Run Step 更新。

### Context Management

Context Management 通过 `SkillMaterializationPort` 或 PromptInjection Command 获取 PromptFragment，并独占注入位置、token budget、去重、缓存分段及与 guidance/memory/AGENTS.md 的顺序。

### Policy / Hook / Audit

Tool BC 不反向调用 Policy 或 Hook。Runtime 在调用 ToolExecutionPort 前后完成评估和通知，并发布审计事件。

### Config

Config 通过只读 ConfigSnapshot 提供 Tool、Skill、Command、MCP 配置。Tool BC 不绕过快照读取裸配置。

### MCP

MCP transport、JSON-RPC、认证和协议 DTO 是 Tool BC 的 adapter 私有实现。MCP Tool 通过 ACL 转换为统一 ToolDescriptor、capabilities、schema 与 ToolOutcome；Runtime 不感知 Tool 来源。

## 6. 设计边界

- **NEVER** 让 Tool、Skill、Command 实现同一执行 trait。
- **NEVER** 向 Runtime 暴露 `Arc<dyn Tool>`、Registry、MCP client 或函数指针。
- **NEVER** 让 Skill 或 Command 直接修改完整 Context Window。
- **NEVER** 让 Slash Command 直接读取 Runtime 内部结构。
- **NEVER** 依赖工具名称黑名单表达授权。
- **MUST** 在 Catalog 与 Execution 两端检查 Scope/Profile。
- **MUST** 让 Tool 调用结果保持领域语义，不依赖 SDK/TUI View。
- **MUST** 将 MCP 未接线能力明确保留为 Target，不假定为现有能力。

## 7. 文档导航

| 文档 | 内容 |
|---|---|
| [01-domain-model.md](01-domain-model.md) | Tool、Catalog、Scope/Profile、Outcome、Skill、Command 的领域模型与不变量 |
| [02-ports-and-lifecycle.md](02-ports-and-lifecycle.md) | 双 Tool 端口、ExecutionScope、取消、Skill/Command 协作、MCP 生命周期 |

## 8. 相关文档

- Runtime 领域模型：[../runtime/01-domain-model.md](../runtime/01-domain-model.md)
- Runtime 端口与装配：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- 迁移治理：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：锁定三类能力边界、Tool 双端口、Scope/Profile 与 MCP 归属 | #787 |
| 2026-07-16 | 冻结 Tool BC Target 目录（v1）：Tool、Skill、Command、MCP 收进私有 `capabilities/` 竖切；端口、模型和协议 detail 归所属切片 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-16 | #972 v2 修订：Hexagonal 成为 crate 内部默认，Target 目录由私有 `capabilities/` 改为 `domain ← application ← ports ← adapters` 依赖方向，MCP 降为 `adapters/mcp/` 子目录 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-17 | #993 完成 Tool crate 物理目录迁移：按证据启用 `domain + adapters` 两层过渡形态，crate-root `lib.rs` 收窄为 façade，`shared/tool` PL 类型迁入 `domain/types`；`application/` / `ports/` 与 T1-T12 语义收口保持 Target 开放 | [#993](https://github.com/rushsinging/aemeath/issues/993) |
