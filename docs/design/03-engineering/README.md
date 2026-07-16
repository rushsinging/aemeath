# 03-engineering · 横切工程

> 层级：03-engineering（横切工程关注点）
> 状态：Target｜Milestone：v0.1.0（S2+ 填充实质内容）
> 本层承载**跨模块的横切关注点**：架构守卫、agent 工程方法论、reasoning graph、可观测性、迁移治理等。不属于单一 BC 的知识落在这里。**设计类文档只描述目标态；迁移治理文档专门承载过渡期的 Current→Target 追踪。**

## 文档索引

| 编号 | 文档 | 内容 |
|---|---|---|
| 01 | [01-architecture-guards.md](01-architecture-guards.md) ✅ | 守卫注册表、依赖铁律拦截、守卫脚本注册表 |
| 02 | [02-agent-orchestration.md](02-agent-orchestration.md) ✅ | Agent 工程五主线（Context / Harness / Loop / Workflow / Graph）、演进决策框架 |
| 03 | [03-migration-governance.md](03-migration-governance.md) ✅ | Current→Target 迁移追踪、Runtime 现状缺口(R1-R10)、死代码退役清单 |
| 04 | [04-testing-and-coverage.md](04-testing-and-coverage.md) ✅ | L0-L5 测试分层、目录组织、fixture/替身、覆盖率、生产可达性、dead-code 与 CI 门禁 |

## 编写原则

- 设计类文档只描述目标态，带"相关文档"链接与"修改历史"。
- **迁移治理**是唯一允许记录 Current 状态的文档，用于追踪旧路径 / 死代码 / 待退役项。

## 相关文档

- 系统级总体设计：[../01-system/](../01-system/)
- 模块级设计：[../02-modules/README.md](../02-modules/README.md)
- 目录总览：[../README.md](../README.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：承接说明 + 待处理事项 | #760 |
| 2026-07-11 | 移除 current 迁移问题描述、把过渡状态收敛到 migration-governance、链接化、新增修改历史 | #760 |
| 2026-07-11 | S2 建 03-migration-governance.md（承接 Runtime 现状缺口），规划表改链接 | #761 |
| 2026-07-14 | 新增测试架构与覆盖率治理，统一 L0-L5、production reachability 与 dead-code 门禁 | [#677](https://github.com/rushsinging/aemeath/issues/677)、[#1006](https://github.com/rushsinging/aemeath/issues/1006) |
