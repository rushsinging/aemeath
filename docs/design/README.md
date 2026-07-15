# docs/design/

> 本目录是 aemeath 的**设计真相源**。自 v0.1.0（#743 DDD 重构）起，采用**三层信息架构**：`01-system` 总体战略 → `02-modules` 模块战术 → `03-engineering` 横切工程。**设计文档只记录目标态（Target），不记录当前代码状态**——旧路径、退役清单与迁移进度集中在 `03-engineering/migration-governance`，避免设计内容与实现现状混淆。

## 三层信息架构

| 层 | 目录 | 承载 |
|---|---|---|
| **01 · 系统级** | [`01-system/`](01-system/) | 产品与领域、统一语言、上下文地图、系统架构、依赖规则、代码组织 |
| **02 · 模块级** | [`02-modules/`](02-modules/) | 各 BC 战术设计（聚合 / 不变量 / 领域服务 / 公开 façade / 真实外部 seam） |
| **03 · 横切** | [`03-engineering/`](03-engineering/) | 架构守卫、agent 工程、reasoning graph、可观测性、迁移治理 |

## 01-system 导航

| 文档 | 角色 |
|---|---|
| [01-product-and-domain.md](01-system/01-product-and-domain.md) | 产品目标、核心问题、子域三分类、15 BC 责任章程与判断规则 |
| [02-ubiquitous-language.md](01-system/02-ubiquitous-language.md) | 统一语言术语表、术语辨析 |
| [03-context-map.md](01-system/03-context-map.md) | 15 BC 集成关系（C/S · ACL · Pub/Sub · OHS · PL · SK）、交付层、Future 预留 |
| [04-system-architecture.md](01-system/04-system-architecture.md) | capability-first 模块化单体 + 选择性 Hexagonal seam + 唯一组合根 + crate 映射 + 传输透明 |
| [05-dependency-rules.md](01-system/05-dependency-rules.md) | Clean 策略 / 细节依赖方向、7 条依赖铁律、跨能力边界、Agent 执行生命周期状态机原则 |
| [06-code-organization.md](01-system/06-code-organization.md) | 系统级代码组织真相：能力优先、用例共置、按需 port 与渐进 crate 边界 |

## 迁移治理入口

旧路径、退役清单与 Current → Target 迁移进度 **MUST** 只记录在 [`03-engineering/migration-governance`](03-engineering/03-migration-governance.md)；本目录的 Target 导航 **NEVER** 复刻这些状态。

## 阅读路径

| 你要做什么 | 先读 |
|---|---|
| 理解 aemeath 是什么、怎么划分 BC | [01-product-and-domain.md](01-system/01-product-and-domain.md) |
| 查术语精确定义 | [02-ubiquitous-language.md](01-system/02-ubiquitous-language.md) |
| 理解 BC 之间怎么集成 / 端口 | [03-context-map.md](01-system/03-context-map.md) |
| 理解 Tool/Skill/Command、Scope/Profile 与 MCP 边界 | [02-modules/tools/](02-modules/tools/README.md) |
| 理解 Provider ACL、统一调用流、reasoning 映射与 Invocation Scope | [02-modules/provider/](02-modules/provider/README.md) |
| 理解持久化、诊断日志与应用自更新的通用域边界 | [Storage](02-modules/storage/README.md) · [Logging](02-modules/logging/README.md) · [Application Version Control](02-modules/application-version-control/README.md) |
| 理解 Memory 检索、注入与 Reflection 引擎 | [02-modules/memory/](02-modules/memory/README.md) |
| 理解 Policy、Hook、Stop/Run Loop 与 Usage Audit 边界 | [02-modules/policy/](02-modules/policy/README.md) + [02-modules/hook/](02-modules/hook/README.md) + [02-modules/audit/](02-modules/audit/README.md) |
| 理解 capability-first 系统形态、选择性 Hexagonal seam 与唯一组合根 | [04-system-architecture.md](01-system/04-system-architecture.md) + [06-code-organization.md](01-system/06-code-organization.md) |
| 决定代码应保持扁平、按用例拆分，或何时引入 model / port / 技术目录 / crate | [06-code-organization.md](01-system/06-code-organization.md) |
| 审查外部 detail、跨能力调用与具体实现装配的依赖方向 | [05-dependency-rules.md](01-system/05-dependency-rules.md) + [06-code-organization.md](01-system/06-code-organization.md) |
| 新增 / 调整架构守卫、Stop 钩子失败排查 | [03-engineering/01-architecture-guards.md](03-engineering/01-architecture-guards.md) |
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

## 相关文档

- 系统架构：[01-system/04-system-architecture.md](01-system/04-system-architecture.md)
- 依赖规则与铁律：[01-system/05-dependency-rules.md](01-system/05-dependency-rules.md)
- 代码组织规范：[01-system/06-code-organization.md](01-system/06-code-organization.md)
- 架构守卫注册表：[03-engineering/01-architecture-guards.md](03-engineering/01-architecture-guards.md)
- 迁移治理：[03-engineering/03-migration-governance.md](03-engineering/03-migration-governance.md)
- 仓库级工作约束：[../../AGENTS.md](../../AGENTS.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 重写为三层信息架构导航 + 迁移地图 + 四态约定 | #760 |
| 2026-07-11 | 迁移地图收敛为过渡说明（迁移追踪归 migration-governance）、状态约定改为纯目标态、新增修改历史 | #760 |
| 2026-07-12 | 阅读路径新增 Tool/Skill/Command 战术设计入口 | #787 |
| 2026-07-12 | 系统导航补充 15 BC 责任章程，并精确状态机原则命名 | #743 / #787 |
| 2026-07-12 | 阅读路径新增 Provider 战术设计入口 | #788 |
| 2026-07-12 | 阅读路径新增 Storage、Logging、Application Version Control 摘要入口 | #793 |
| 2026-07-12 | 阅读路径新增 Memory 战术设计入口 | #789 |
| 2026-07-12 | 阅读路径新增 Policy/Hook/Audit 战术设计入口 | #790 |
| 2026-07-12 | 三层设计导航交叉引用收敛，并统一指向对应 Target 文档 | #743 |
| 2026-07-14 | 注册系统级代码组织真相源，更新架构与依赖阅读路径，移除 Target 导航中的迁移进度复刻 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
