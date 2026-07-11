# docs/design/

> 本目录是 aemeath 的**设计真相源**。自 v0.1.0（#743 DDD 重构）起，采用**三层信息架构**：`01-system` 总体战略 → `02-modules` 模块战术 → `03-engineering` 横切工程。**设计文档只记录目标态（Target），不记录当前代码状态**——过渡期的旧文档去向与迁移追踪集中在 `03-engineering/migration-governance`，避免设计内容与实现现状混淆。

## 三层信息架构

| 层 | 目录 | 承载 | 进度 |
|---|---|---|---|
| **01 · 系统级** | [`01-system/`](01-system/) | 产品与领域、统一语言、上下文地图、系统架构、依赖规则 | ✅ S1 已落地 |
| **02 · 模块级** | [`02-modules/`](02-modules/) | 各 BC 战术设计（聚合 / 不变量 / 领域服务 / 模块端口） | 🚧 S2 填充 |
| **03 · 横切** | [`03-engineering/`](03-engineering/) | 架构守卫、agent 工程、reasoning graph、可观测性、迁移治理 | 🚧 S2+ 填充 |

## 01-system 导航

| 文档 | 角色 |
|---|---|
| [01-product-and-domain.md](01-system/01-product-and-domain.md) | 产品目标、核心问题、子域三分类、15 BC 清单、关键约束 |
| [02-ubiquitous-language.md](01-system/02-ubiquitous-language.md) | 统一语言术语表、术语辨析 |
| [03-context-map.md](01-system/03-context-map.md) | 15 BC 集成关系（C/S · ACL · Pub/Sub · OHS · PL · SK）、交付层、Future 预留 |
| [04-system-architecture.md](01-system/04-system-architecture.md) | 模块化单体 + Hexagonal + 组合根 + crate 映射 + 传输透明 |
| [05-dependency-rules.md](01-system/05-dependency-rules.md) | Clean 依赖方向、7 条依赖铁律、COLA 重定位、单状态机原则 |

## 旧文档过渡说明

目录内仍存在旧的扁平文档（`01-outline.md` ~ `07-server-design.md`）与 [`02-architecture-guards.md`](02-architecture-guards.md)。它们正逐步迁入三层结构：

- **迁移去向、旧路径追踪、退役清单** 统一在 `03-engineering/migration-governance`（S2 建立）维护，**不在本 README 记录 current 状态**。
- [`02-architecture-guards.md`](02-architecture-guards.md) 在迁移完成前**保持原位**（CLAUDE.md 触发表引用该路径），迁移时同步更新触发表。

## 阅读路径

| 你要做什么 | 先读 |
|---|---|
| 理解 aemeath 是什么、怎么划分 BC | [01-product-and-domain.md](01-system/01-product-and-domain.md) |
| 查术语精确定义 | [02-ubiquitous-language.md](01-system/02-ubiquitous-language.md) |
| 理解 BC 之间怎么集成 / 端口 | [03-context-map.md](01-system/03-context-map.md) |
| 理解依赖方向 / 六边形 | [04-system-architecture.md](01-system/04-system-architecture.md) + [05-dependency-rules.md](01-system/05-dependency-rules.md) |
| 新增 / 调整架构守卫、Stop 钩子失败排查 | [02-architecture-guards.md](02-architecture-guards.md) |
| 理解 DDD 方法论 + aemeath 实战案例 | [../DDD/index.html](../DDD/index.html) |

## 状态约定

- **设计文档（01-system / 02-modules / 03-engineering 的设计类文档）只记录目标态（Target）**，必要时用 **Decision** 标注关键决策与被否决的替代方案。
- **NEVER** 在设计文档记录 Current（当前代码落点 / 命名 / 现状缺陷）——避免代码演进后文档过时造成混淆。
- 需要追踪现状的场景（旧路径、死代码、迁移进度）**MUST** 集中到 `03-engineering/migration-governance`，那是唯一允许记录 Current 的文档。

## 维护规则

- 每篇文档 **MUST** 带"相关文档"链接与"修改历史"章节。
- 任何修改 **MUST** 走 worktree + PR（见 [AGENTS.md](../../AGENTS.md) §Git 工作流）。
- 新增 / 撤销文档时 **MUST** 更新本 README 的导航。
- 迁移旧文档时 **MUST** 同步更新 CLAUDE.md 触发表中对应的路径引用，避免断链。

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 重写为三层信息架构导航 + 迁移地图 + 四态约定 | #760 |
| 2026-07-11 | 迁移地图收敛为过渡说明（迁移追踪归 migration-governance）、状态约定改为纯目标态、新增修改历史 | #760 |
