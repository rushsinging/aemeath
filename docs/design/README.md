# docs/design/

> 本目录是 aemeath 的**设计真相源**。自 v0.1.0（#743 DDD 重构）起，采用**三层信息架构**：`01-system` 总体战略 → `02-modules` 模块战术 → `03-engineering` 横切工程。旧的扁平文档（`01-outline` ~ `07-server-design`）正逐步迁入三层结构（见下方迁移地图）。任何涉及架构不变量的代码 / 配置修改，**MUST** 核对相关文档并保持一致。

## 三层信息架构

| 层 | 目录 | 承载 | 状态 |
|---|---|---|---|
| **01 · 系统级** | [`01-system/`](01-system/) | 产品与领域、统一语言、上下文地图、系统架构、依赖规则 | ✅ S1 已建 |
| **02 · 模块级** | [`02-modules/`](02-modules/) | 各 BC 战术设计（聚合 / 不变量 / 领域服务 / 模块端口） | 🚧 S2 填充 |
| **03 · 横切** | [`03-engineering/`](03-engineering/) | 架构守卫、agent 工程、reasoning graph、可观测性、迁移治理 | 🚧 S2+ 填充 |

## 01-system 导航（S1 已落地）

| 文档 | 角色 |
|---|---|
| [01-product-and-domain.md](01-system/01-product-and-domain.md) | 产品目标、核心问题、子域三分类、15 BC 清单、关键约束 |
| [02-ubiquitous-language.md](01-system/02-ubiquitous-language.md) | 统一语言术语表（含当前代码命名与迁移映射）、术语辨析 |
| [03-context-map.md](01-system/03-context-map.md) | 15 BC 集成关系（C/S · ACL · Pub/Sub · OHS · PL · SK）、交付层、Future 预留 |
| [04-system-architecture.md](01-system/04-system-architecture.md) | 模块化单体 + Hexagonal + Composition Root + crate 映射 + 传输透明 |
| [05-dependency-rules.md](01-system/05-dependency-rules.md) | Clean 依赖方向、7 条依赖铁律、COLA 重定位、单状态机原则 |

## 现有文档迁移地图（S1 只标去向，不移动文件）

| 现有文档 | 状态 | 迁移去向 | 执行阶段 |
|---|---|---|---|
| [01-outline.md](01-outline.md) | 待归档 | 内容重写进 `01-system/*`，完成后归档 | S2 |
| [02-architecture-guards.md](02-architecture-guards.md) | 保持原位 | → `03-engineering/architecture-guards`（CLAUDE.md 触发表引用，暂不动） | S2/S7 |
| [03-runtime-design.md](03-runtime-design.md) | 待拆分 | → `02-modules/runtime` + `02-modules/context-management`（抽出 2 个内嵌重构 spec） | S2 |
| [04-tui-design.md](04-tui-design.md) | 待迁移 | → `02-modules/tui` | S2 |
| [05-agent-orchestration.md](05-agent-orchestration.md) | 知识储备 | → `03-engineering/agent-engineering` | S2 |
| [06-agent-reasoning-graph.md](06-agent-reasoning-graph.md) | 待对齐 | → `03-engineering/reasoning-graph`（**先解决 doc-vs-code 分歧**） | S2 |
| [07-server-design.md](07-server-design.md) | 草案 | → `02-modules/server` | S2 |
| [docs/file-split-plan.md](../file-split-plan.md) | 参考 | → `03-engineering/migration-governance` 整合 | S5 |

## 阅读路径

| 你要做什么 | 先读 |
|---|---|
| 理解 aemeath 是什么、怎么划分 BC | [01-system/01-product-and-domain.md](01-system/01-product-and-domain.md) |
| 查术语精确定义 | [01-system/02-ubiquitous-language.md](01-system/02-ubiquitous-language.md) |
| 理解 BC 之间怎么集成 / 端口 | [01-system/03-context-map.md](01-system/03-context-map.md) |
| 理解依赖方向 / 六边形 | [01-system/04-system-architecture.md](01-system/04-system-architecture.md) + [05-dependency-rules.md](01-system/05-dependency-rules.md) |
| 新增 / 调整架构守卫、Stop 钩子失败排查 | [02-architecture-guards.md](02-architecture-guards.md) |
| 实现 / 改 Runtime、TUI、Reasoning Graph | 暂读现有 `03`/`04`/`06`，S2 迁入 `02-modules/` 后改读新路径 |
| 理解 DDD 方法论 + aemeath 实战案例 | [../DDD/index.html](../DDD/index.html) |

## 状态约定

文档正文用四态区分内容性质（S1 起）：

- **Current**：如实描述当前代码事实（可能含缺陷 / 不一致），只写已核实的。
- **Target**：目标设计，MUST 达成的形态。
- **Migration**：从 Current 到 Target 的迁移路径与阶段。
- **Decision**：关键决策与理由（含被否决的替代方案）。

旧文档沿用的索引状态标签（已落地 / 演进中 / 草案 / 知识储备）在迁移完成前继续有效。

## 维护规则

- 文档与代码冲突时**以代码为准**，但 **MUST** 在同一 PR 同步更新文档。
- 任何修改 **MUST** 走 worktree + PR（见 [AGENTS.md](../../AGENTS.md) §Git 工作流）。
- 新增 / 撤销文档时 **MUST** 更新本 README 的导航与迁移地图。
- 迁移旧文档时 **MUST** 同步更新 CLAUDE.md 触发表中对应的路径引用，避免断链。
